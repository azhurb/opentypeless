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
    CharsAndNewlines { max_chars: usize, max_newlines: usize },
}

/// Decide how to split `text` based on the focused app.
pub fn plan_chunks(text: String, app: &AppContext) -> ChunkPlan {
    let chunks = match chunk_limit_for(app) {
        ChunkLimit::None => return ChunkPlan::Single(text),
        ChunkLimit::Chars(max) => chunk_by_chars(&text, max, None),
        ChunkLimit::CharsAndNewlines { max_chars, max_newlines } => {
            chunk_by_chars(&text, max_chars, Some(max_newlines))
        }
    };
    match chunks.len() {
        0 => ChunkPlan::Single(String::new()),
        1 => ChunkPlan::Single(chunks.into_iter().next().unwrap()),
        _ => ChunkPlan::Multi(chunks),
    }
}

/// Decide the per-target chunking strategy.
///
/// Terminal-hosted CLIs are detected by matching the foreground app's
/// bundle ID against a known list of terminal emulators and editor
/// terminal panels, then matching the window title against a CLI name.
/// Constants per CLI are empirical: Claude prefers small chunks with
/// few newlines per chunk; Codex tolerates larger chunks with no
/// newline limit; Gemini is treated like Codex pending confirmation.
fn chunk_limit_for(app: &AppContext) -> ChunkLimit {
    let bundle_id = match app.bundle_id.as_deref() {
        Some(id) => id,
        None => return ChunkLimit::None,
    };
    if !is_terminal_like(bundle_id) {
        return ChunkLimit::None;
    }
    let title_lc = app.window_title.to_lowercase();
    if title_lc.contains("claude") {
        return ChunkLimit::CharsAndNewlines { max_chars: 800, max_newlines: 2 };
    }
    if title_lc.contains("codex") {
        return ChunkLimit::Chars(1000);
    }
    if title_lc.contains("gemini") {
        return ChunkLimit::Chars(1000);
    }
    ChunkLimit::None
}

/// Bundle IDs we treat as "terminal-like" for the purpose of CLI detection.
/// Includes pure terminal emulators (Terminal.app, iTerm2, Ghostty, …)
/// plus editors and IDEs that host an integrated terminal panel where a
/// CLI may be running (VS Code, Cursor, IntelliJ family).
fn is_terminal_like(bundle_id: &str) -> bool {
    matches!(
        bundle_id,
        // Pure terminal emulators
        "com.apple.Terminal"
        | "com.googlecode.iterm2"
        | "dev.warp.Warp-Stable"
        | "com.mitchellh.ghostty"
        | "net.kovidgoyal.kitty"
        | "io.alacritty"
        | "org.alacritty"
        | "co.zeit.hyper"
        | "com.github.wez.wezterm"
        // Editors / IDEs with integrated terminal panels
        | "com.microsoft.VSCode"
        | "com.todesktop.230313mzl4w4u92"   // Cursor
        | "com.exafunction.windsurf"
        | "com.jetbrains.intellij"
        | "com.jetbrains.intellij.ce"
        | "com.jetbrains.pycharm"
        | "com.jetbrains.pycharm.ce"
        | "com.jetbrains.WebStorm"
        | "com.jetbrains.webstorm"
        | "com.jetbrains.rubymine"
        | "com.jetbrains.datagrip"
        | "com.jetbrains.goland"
        | "com.jetbrains.rider"
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
        max_newlines.map_or(true, |m| current_nls + add_nls <= m)
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
        }
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
        match plan_chunks("hello".repeat(500), &app) {
            ChunkPlan::Single(_) => {}
            ChunkPlan::Multi(_) => panic!("expected Single when bundle_id is None"),
        }
    }

    #[test]
    fn no_chunking_for_non_terminal_app() {
        let app = app_with(Some("com.apple.Notes"), "Codex");
        match plan_chunks("a".repeat(2000), &app) {
            ChunkPlan::Single(_) => {}
            ChunkPlan::Multi(_) => panic!("Notes should not chunk"),
        }
    }

    #[test]
    fn no_chunking_for_terminal_without_known_cli() {
        let app = app_with(Some("com.googlecode.iterm2"), "user@host: ~");
        match plan_chunks("a".repeat(2000), &app) {
            ChunkPlan::Single(_) => {}
            ChunkPlan::Multi(_) => panic!("plain shell session should not chunk"),
        }
    }

    #[test]
    fn chunks_for_codex_in_iterm2() {
        let app = app_with(Some("com.googlecode.iterm2"), "codex — main");
        match plan_chunks("a".repeat(2500), &app) {
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
        match plan_chunks("line\n".repeat(300), &app) {
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
        match plan_chunks("a".repeat(1500), &app) {
            ChunkPlan::Multi(_) => {}
            ChunkPlan::Single(_) => panic!("title match must be case-insensitive"),
        }
    }

    #[test]
    fn short_text_stays_single_even_for_known_cli() {
        let app = app_with(Some("com.googlecode.iterm2"), "codex");
        match plan_chunks("hi".to_string(), &app) {
            ChunkPlan::Single(s) => assert_eq!(s, "hi"),
            ChunkPlan::Multi(_) => panic!("short text should not chunk"),
        }
    }
}
