use crate::app_detector::cli_detect::{CliKind, Confidence, DetectedCli};
use crate::app_detector::AppContext;

/// How a single paste should be delivered to the foreground app.
///
/// Most apps handle a single bulk paste fine. Some terminal-hosted CLIs
/// (Claude CLI, Codex CLI, Gemini CLI) drop characters or mis-parse large
/// pastes — for those, we split the text into chunks and paste each
/// chunk separately with brief delays.
pub enum ChunkPlan {
    Single(String),
    Multi(Vec<String>),
}

enum ChunkLimit {
    None,
    Chars(usize),
    CharsAndNewlines {
        max_chars: usize,
        max_newlines: usize,
    },
}

/// Decide how to split `text` based on the focused app and any coding CLI
/// detected running inside it.
pub fn plan_chunks(text: String, app: &AppContext, detected: Option<DetectedCli>) -> ChunkPlan {
    let chunks = match chunk_limit_for(app, detected) {
        ChunkLimit::None => return ChunkPlan::Single(text),
        ChunkLimit::Chars(max) => chunk_by_chars(&text, max, None),
        ChunkLimit::CharsAndNewlines {
            max_chars,
            max_newlines,
        } => chunk_by_chars(&text, max_chars, Some(max_newlines)),
    };
    match chunks.len() {
        0 => ChunkPlan::Single(String::new()),
        1 => ChunkPlan::Single(chunks.into_iter().next().unwrap()),
        _ => ChunkPlan::Multi(chunks),
    }
}

/// Decide the per-target chunking strategy. Constants per CLI are empirical:
/// Claude prefers small chunks with few newlines per chunk; Codex tolerates
/// larger chunks with no newline limit; Gemini is treated like Codex pending
/// confirmation.
///
/// Three ways to recognize a terminal-hosted CLI, in descending order of how
/// much they tell us about *this* window:
///
/// 1. **Process descendancy (`High`)**: a coding CLI running inside the focused
///    app's process tree. Host-independent — it works even when the window
///    title doesn't name the CLI (e.g. an IDE's integrated terminal, which
///    reports the project name).
/// 2. **Window title**: the foreground bundle is a known terminal/IDE and its
///    title contains the CLI name. Beats `Low` below, because a title describes
///    the focused window while `Low` only describes the machine.
/// 3. **A CLI running somewhere, into a pure terminal (`Low`)**: see below.
///
/// Requiring descendancy alone was too strict, and that is the bug this arm
/// fixes. A session-persistence daemon owning the pty breaks the parent chain,
/// so the CLI is no longer a descendant of the terminal you are looking at:
///
/// - iTerm2 parents shells to `iTermServer` so sessions survive a restart. That
///   server is re-parented to launchd once iTerm2 is relaunched, at which point
///   nothing under it descends from the iTerm2 app.
/// - Herdr (<https://herdr.dev>) does this deliberately: a background runtime
///   owns the agent's pane so it outlives detach, network drops and reboots.
/// - `tmux` and `screen` have the same shape.
///
/// In all of those, detection degrades to `Low` and the paste went unchunked,
/// which is exactly when it is too big: Claude Code replaces any paste over
/// `800` characters, or with more than 2 newlines, with a `[Pasted text #N]`
/// placeholder. So `Low` into a terminal-like app now chunks too. The trade is
/// deliberately asymmetric — a false positive costs a paste split into pieces
/// that a plain shell handles identically, while a false negative costs the
/// user a collapsed placeholder instead of their words.
fn chunk_limit_for(app: &AppContext, detected: Option<DetectedCli>) -> ChunkLimit {
    if let Some(DetectedCli {
        kind,
        confidence: Confidence::High,
    }) = detected
    {
        return cli_chunk_limit(kind);
    }

    let bundle_id = match app.bundle_id.as_deref() {
        Some(id) => id,
        None => return ChunkLimit::None,
    };
    if !is_terminal_like(bundle_id) {
        return ChunkLimit::None;
    }
    let title_lc = app.window_title.to_lowercase();
    if title_lc.contains("claude") {
        return cli_chunk_limit(CliKind::Claude);
    }
    if title_lc.contains("codex") {
        return cli_chunk_limit(CliKind::Codex);
    }
    if title_lc.contains("gemini") {
        return cli_chunk_limit(CliKind::Gemini);
    }
    if is_pure_terminal(bundle_id)
        && matches!(
            detected,
            Some(DetectedCli {
                confidence: Confidence::Low,
                ..
            })
        )
    {
        return strictest_cli_limit();
    }
    ChunkLimit::None
}

/// The limit to use when we know a coding CLI is running and the paste target
/// is a terminal, but not which CLI owns *this* window.
///
/// Deliberately ignores the `kind` that came back with `Low`. That kind is
/// whichever matching process the scan happened to reach first, so with both
/// Claude Code and Codex running it is a coin flip — and picking wrong is not
/// symmetric: Codex's limit (1000 chars, no newline cap) is looser than
/// Claude's and would sail straight past the threshold that collapses a Claude
/// paste. Guessing costs nothing here, so guess the strictest.
fn strictest_cli_limit() -> ChunkLimit {
    cli_chunk_limit(CliKind::Claude)
}

/// Chunk limits per CLI. Claude's are exact rather than empirical: its TUI
/// collapses a paste into a `[Pasted text #N +M lines]` placeholder when the
/// pasted string is longer than 800 characters or carries more than 2 newlines,
/// so staying at or under both is what keeps a dictation visible as text. Codex
/// and Gemini tolerate up to ~1000 chars; those two remain empirical.
///
/// One known gap, inferred and unverified: Claude's newline threshold appears
/// to tighten in a very short terminal (the limit reads as `min(rows - 10, 2)`),
/// so a pane under ~12 rows can collapse a chunk this function considers safe.
fn cli_chunk_limit(kind: CliKind) -> ChunkLimit {
    match kind {
        CliKind::Claude => ChunkLimit::CharsAndNewlines {
            max_chars: 800,
            max_newlines: 2,
        },
        CliKind::Codex | CliKind::Gemini => ChunkLimit::Chars(1000),
    }
}

/// Bundle IDs whose *only* surface is a terminal. Every paste into one of these
/// goes to a shell, which is what lets the `Low` arm of [`chunk_limit_for`]
/// chunk on machine-wide evidence: the worst case is a shell receiving text in
/// several pieces instead of one, which it handles identically.
///
/// Deliberately excludes IDEs with an integrated terminal panel. There, the
/// likelier paste target is the editor, and splitting a dictation across it
/// would leave several undo steps for one utterance — a real cost paid on a
/// guess. Those still chunk on a `High` detection or a title match, both of
/// which are evidence about the focused window rather than the machine.
pub(crate) fn is_pure_terminal(bundle_id: &str) -> bool {
    matches!(
        bundle_id,
        "com.apple.Terminal"
            | "com.googlecode.iterm2"
            | "dev.warp.Warp-Stable"
            | "com.mitchellh.ghostty"
            | "net.kovidgoyal.kitty"
            | "io.alacritty"
            | "org.alacritty"
            | "co.zeit.hyper"
            | "com.github.wez.wezterm"
    )
}

/// Bundle IDs we treat as "terminal-like" for the purpose of CLI detection:
/// pure terminal emulators plus editors and IDEs that host an integrated
/// terminal panel where a CLI may be running (VS Code, Cursor, IntelliJ).
pub(crate) fn is_terminal_like(bundle_id: &str) -> bool {
    is_pure_terminal(bundle_id)
        || matches!(
            bundle_id,
            "com.microsoft.VSCode"
        | "com.todesktop.230313mzl4w4u92"   // Cursor
        | "com.exafunction.windsurf"
        | "com.jetbrains.intellij"
        | "com.jetbrains.intellij.ce"
        | "com.jetbrains.pycharm"
        | "com.jetbrains.pycharm.ce"
        | "com.jetbrains.PhpStorm"
        | "com.jetbrains.WebStorm"
        | "com.jetbrains.webstorm"
        | "com.jetbrains.rubymine"
        | "com.jetbrains.datagrip"
        | "com.jetbrains.goland"
        | "com.jetbrains.rider"
        | "com.jetbrains.CLion"
        | "com.jetbrains.clion"
        | "com.jetbrains.rustrover"
        | "com.jetbrains.RustRover"
            | "com.google.android.studio"
        )
}

/// Split `text` into chunks of at most `max_chars` Unicode characters each,
/// optionally also enforcing at most `max_newlines` line breaks per chunk.
/// Prefers line-boundary splits; falls back to splitting mid-line at char
/// boundaries only when a single line is itself longer than `max_chars`.
fn chunk_by_chars(text: &str, max_chars: usize, max_newlines: Option<usize>) -> Vec<String> {
    debug_assert!(max_chars > 0, "max_chars must be > 0");
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_chars: usize = 0;
    let mut current_newlines: usize = 0;

    let newline_room = |current_nls: usize, add_nls: usize| -> bool {
        max_newlines.is_none_or(|m| current_nls + add_nls <= m)
    };

    for line in text.split_inclusive('\n') {
        let line_chars = line.chars().count();
        let line_nls = if line.ends_with('\n') { 1 } else { 0 };

        if current_chars + line_chars <= max_chars && newline_room(current_newlines, line_nls) {
            current.push_str(line);
            current_chars += line_chars;
            current_newlines += line_nls;
            continue;
        }

        if !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }

        if line_chars <= max_chars && newline_room(0, line_nls) {
            current.push_str(line);
            current_chars = line_chars;
            current_newlines = line_nls;
            continue;
        }

        let mut buf = String::new();
        let mut buf_chars: usize = 0;
        for ch in line.chars() {
            if buf_chars + 1 > max_chars {
                chunks.push(std::mem::take(&mut buf));
                buf_chars = 0;
            }
            buf.push(ch);
            buf_chars += 1;
        }
        current = buf;
        current_chars = buf_chars;
        current_newlines = if current.ends_with('\n') { 1 } else { 0 };
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::AppType;

    fn app_with(bundle_id: Option<&str>, window_title: &str) -> AppContext {
        AppContext {
            app_name: String::new(),
            window_title: window_title.to_string(),
            app_type: AppType::General,
            bundle_id: bundle_id.map(|s| s.to_string()),
            pid: None,
        }
    }

    fn high(kind: CliKind) -> Option<DetectedCli> {
        Some(DetectedCli {
            kind,
            confidence: Confidence::High,
        })
    }

    fn low(kind: CliKind) -> Option<DetectedCli> {
        Some(DetectedCli {
            kind,
            confidence: Confidence::Low,
        })
    }

    /// Every chunk must survive Claude Code's own collapse rule: a paste is
    /// replaced by `[Pasted text #N +M lines]` above 800 characters or more
    /// than 2 newlines. Asserting the rule directly keeps the constants honest.
    fn assert_survives_claude_paste_collapse(chunks: &[String]) {
        for (i, c) in chunks.iter().enumerate() {
            assert!(
                c.chars().count() <= 800,
                "chunk {i} is {} chars, Claude collapses above 800",
                c.chars().count()
            );
            assert!(
                c.matches('\n').count() <= 2,
                "chunk {i} has {} newlines, Claude collapses above 2",
                c.matches('\n').count()
            );
        }
    }

    /// The reported bug. A session-persistence daemon owns the pty — iTerm2's
    /// `iTermServer` after a restart, Herdr's background runtime, tmux — so the
    /// CLI is not a descendant of the focused terminal and detection lands on
    /// `Low`. That used to mean no chunking at all, and a minute of dictation
    /// (comfortably over 800 characters) arrived as a single paste that Claude
    /// Code collapsed into a placeholder.
    #[test]
    fn low_confidence_in_a_terminal_still_chunks() {
        let app = app_with(Some("com.googlecode.iterm2"), "~/projects/opentypeless");
        let text = "word ".repeat(400); // 2000 chars, no newlines
        match plan_chunks(text.clone(), &app, low(CliKind::Claude)) {
            ChunkPlan::Multi(chunks) => {
                assert!(chunks.len() > 1);
                assert_eq!(chunks.concat(), text);
                assert_survives_claude_paste_collapse(&chunks);
            }
            ChunkPlan::Single(_) => panic!("a 2000-char paste must not go in one piece"),
        }
    }

    /// With both Claude Code and Codex running, the `kind` attached to `Low` is
    /// whichever process the scan reached first. Codex's looser limit would let
    /// a 1000-char chunk through and Claude would collapse it, so the fallback
    /// must ignore the coin flip and use the strictest limit.
    #[test]
    fn low_confidence_ignores_the_reported_kind_and_uses_the_strictest_limit() {
        let app = app_with(Some("com.googlecode.iterm2"), "~/projects/opentypeless");
        let text = "x".repeat(900);
        for kind in [CliKind::Claude, CliKind::Codex, CliKind::Gemini] {
            match plan_chunks(text.clone(), &app, low(kind)) {
                ChunkPlan::Multi(chunks) => assert_survives_claude_paste_collapse(&chunks),
                ChunkPlan::Single(c) => {
                    panic!("{kind:?}: 900 chars went unchunked ({} chars)", c.len())
                }
            }
        }
    }

    /// `Low` says "a CLI is running on this machine", which is only evidence
    /// about the paste target when that target is a terminal. A browser or a
    /// text editor must still get one clean paste.
    #[test]
    fn low_confidence_outside_a_terminal_does_not_chunk() {
        for bundle in [Some("com.apple.Safari"), Some("com.apple.TextEdit"), None] {
            let app = app_with(bundle, "some window");
            let text = "x".repeat(5000);
            assert!(
                matches!(
                    plan_chunks(text, &app, low(CliKind::Claude)),
                    ChunkPlan::Single(_)
                ),
                "{bundle:?} is not a terminal and must not be chunked"
            );
        }
    }

    /// A title naming the CLI describes the focused window; `Low` only
    /// describes the machine. The title must win.
    #[test]
    fn window_title_beats_low_confidence() {
        let app = app_with(Some("com.googlecode.iterm2"), "codex — ~/src");
        let text = "x".repeat(950);
        match plan_chunks(text, &app, low(CliKind::Claude)) {
            // Codex tolerates 1000 chars, so a 950-char paste stays whole.
            ChunkPlan::Single(c) => assert_eq!(c.chars().count(), 950),
            ChunkPlan::Multi(_) => panic!("title said codex, which tolerates 950 chars"),
        }
    }

    #[test]
    fn pure_terminals_are_a_strict_subset_of_terminal_like() {
        for id in [
            "com.googlecode.iterm2",
            "com.apple.Terminal",
            "com.mitchellh.ghostty",
        ] {
            assert!(is_pure_terminal(id), "{id} should be a pure terminal");
            assert!(is_terminal_like(id), "{id} should also be terminal-like");
        }
        for id in ["com.microsoft.VSCode", "com.jetbrains.PhpStorm"] {
            assert!(
                !is_pure_terminal(id),
                "{id} hosts an editor, not just a shell"
            );
            assert!(
                is_terminal_like(id),
                "{id} still hosts an integrated terminal"
            );
        }
    }

    /// A dictation short enough to be safe must still arrive as one paste, or
    /// every ordinary sentence pays the chunking latency.
    #[test]
    fn short_dictation_is_not_split_by_the_low_path() {
        let app = app_with(Some("com.googlecode.iterm2"), "~/projects");
        let text = "This is a short dictated sentence.".to_string();
        assert!(matches!(
            plan_chunks(text, &app, low(CliKind::Claude)),
            ChunkPlan::Single(_)
        ));
    }

    #[test]
    fn short_text_one_chunk() {
        let out = chunk_by_chars("hello world", 100, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], "hello world");
    }

    #[test]
    fn empty_text_one_empty_chunk() {
        let out = chunk_by_chars("", 100, None);
        assert_eq!(out, vec![String::new()]);
    }

    #[test]
    fn splits_at_line_boundary() {
        let out = chunk_by_chars("aaaa\nbbbb\ncccc", 5, None);
        assert!(out.len() >= 2);
        let rejoined: String = out.concat();
        assert_eq!(rejoined, "aaaa\nbbbb\ncccc");
    }

    #[test]
    fn long_single_line_splits_mid_line() {
        let text = "x".repeat(2500);
        let out = chunk_by_chars(&text, 1000, None);
        assert_eq!(out.len(), 3);
        assert!(out[0].chars().count() <= 1000);
        assert_eq!(out.concat(), text);
    }

    #[test]
    fn enforces_max_newlines() {
        let text = "a\nb\nc\nd\ne\nf";
        let out = chunk_by_chars(text, 1000, Some(2));
        for c in &out {
            assert!(
                c.matches('\n').count() <= 2,
                "chunk has too many newlines: {c:?}"
            );
        }
        assert_eq!(out.concat(), text);
    }

    #[test]
    fn handles_multibyte_chars_safely() {
        let text = "héllo wörld ☃ 😀 ".repeat(80);
        let out = chunk_by_chars(&text, 50, None);
        for c in &out {
            assert!(
                c.is_char_boundary(0) && c.is_char_boundary(c.len()),
                "chunk has invalid UTF-8 boundary: {c:?}"
            );
            assert!(c.chars().count() <= 50, "chunk over budget: {c:?}");
        }
        assert_eq!(out.concat(), text);
    }

    #[test]
    fn no_chunking_when_bundle_id_absent() {
        let app = app_with(None, "Codex");
        match plan_chunks("hello".repeat(500), &app, None) {
            ChunkPlan::Single(_) => {}
            ChunkPlan::Multi(_) => panic!("expected Single when bundle_id is None"),
        }
    }

    #[test]
    fn no_chunking_for_non_terminal_app() {
        let app = app_with(Some("com.apple.Notes"), "Codex");
        match plan_chunks("a".repeat(2000), &app, None) {
            ChunkPlan::Single(_) => {}
            ChunkPlan::Multi(_) => panic!("Notes should not chunk"),
        }
    }

    #[test]
    fn no_chunking_for_terminal_without_known_cli() {
        let app = app_with(Some("com.googlecode.iterm2"), "user@host: ~");
        match plan_chunks("a".repeat(2000), &app, None) {
            ChunkPlan::Single(_) => {}
            ChunkPlan::Multi(_) => panic!("plain shell session should not chunk"),
        }
    }

    #[test]
    fn chunks_for_codex_in_iterm2() {
        let app = app_with(Some("com.googlecode.iterm2"), "codex — main");
        match plan_chunks("a".repeat(2500), &app, None) {
            ChunkPlan::Multi(chunks) => {
                assert!(chunks.len() >= 3, "expected ≥3 chunks for 2500-char paste");
                for c in &chunks {
                    assert!(c.chars().count() <= 1000);
                }
            }
            ChunkPlan::Single(_) => panic!("expected Multi for Codex with long input"),
        }
    }

    #[test]
    fn chunks_for_claude_in_intellij_terminal() {
        let app = app_with(
            Some("com.jetbrains.intellij"),
            "Claude — opentypeless [~/projects/opentypeless]",
        );
        match plan_chunks("line\n".repeat(300), &app, None) {
            ChunkPlan::Multi(chunks) => {
                for c in &chunks {
                    assert!(c.chars().count() <= 800);
                    assert!(c.matches('\n').count() <= 2);
                }
            }
            ChunkPlan::Single(_) => panic!("expected Multi for Claude with long input"),
        }
    }

    #[test]
    fn cli_match_is_case_insensitive() {
        let app = app_with(Some("com.googlecode.iterm2"), "CODEX — main");
        match plan_chunks("a".repeat(1500), &app, None) {
            ChunkPlan::Multi(_) => {}
            ChunkPlan::Single(_) => panic!("title match must be case-insensitive"),
        }
    }

    #[test]
    fn short_text_stays_single_even_for_known_cli() {
        let app = app_with(Some("com.googlecode.iterm2"), "codex");
        match plan_chunks("hi".to_string(), &app, None) {
            ChunkPlan::Single(s) => assert_eq!(s, "hi"),
            ChunkPlan::Multi(_) => panic!("short text should not chunk"),
        }
    }

    // The regression case: a JetBrains IDE (PhpStorm) whose bundle id isn't in
    // the terminal allowlist and whose window title doesn't name the CLI. The
    // title-based arm can't fire, so only high-confidence process detection
    // (arm A) keeps the long paste from being delivered as one bulk Cmd+V.
    #[test]
    fn high_confidence_claude_chunks_even_when_title_and_bundle_miss() {
        let app = app_with(Some("com.jetbrains.PhpStorm"), "");
        match plan_chunks("line\n".repeat(300), &app, high(CliKind::Claude)) {
            ChunkPlan::Multi(chunks) => {
                for c in &chunks {
                    assert!(c.chars().count() <= 800);
                    assert!(c.matches('\n').count() <= 2);
                }
            }
            ChunkPlan::Single(_) => {
                panic!("high-confidence Claude detection must chunk regardless of title/bundle")
            }
        }
    }

    #[test]
    fn high_confidence_codex_uses_codex_limits() {
        let app = app_with(Some("com.jetbrains.PhpStorm"), "");
        match plan_chunks("a".repeat(2500), &app, high(CliKind::Codex)) {
            ChunkPlan::Multi(chunks) => {
                for c in &chunks {
                    assert!(c.chars().count() <= 1000);
                }
            }
            ChunkPlan::Single(_) => panic!("expected Multi for high-confidence Codex"),
        }
    }

    // Low confidence (a CLI running, but not under the focused app) must NOT
    // trigger arm A. PhpStorm is terminal-*like* but not a pure terminal, so
    // the machine-wide `Low` arm is skipped too: the likelier target is the
    // editor, and a title match here can't fire. The paste stays one event.
    #[test]
    fn low_confidence_does_not_trigger_arm_a() {
        let app = app_with(Some("com.jetbrains.PhpStorm"), "");
        let low = Some(DetectedCli {
            kind: CliKind::Claude,
            confidence: Confidence::Low,
        });
        match plan_chunks("line\n".repeat(300), &app, low) {
            ChunkPlan::Single(_) => {}
            ChunkPlan::Multi(_) => panic!("low confidence must not chunk via arm A"),
        }
    }

    // Arm A wins over arm B: even in a recognized terminal, high-confidence
    // detection should set the strategy without needing a title match.
    #[test]
    fn arm_a_fires_in_plain_terminal_without_title_match() {
        let app = app_with(Some("com.googlecode.iterm2"), "user@host: ~");
        match plan_chunks("line\n".repeat(300), &app, high(CliKind::Claude)) {
            ChunkPlan::Multi(_) => {}
            ChunkPlan::Single(_) => panic!("arm A should fire even without a title match"),
        }
    }
}
