//! Strips reasoning blocks out of a model's visible output.
//!
//! Reasoning models emit their scratchpad as `<think>…</think>` inside the
//! ordinary `content` field whenever the provider is in "raw" reasoning mode —
//! Groq's default for `qwen/qwen3.6-27b`, and the norm for DeepSeek-R1, GLM and
//! most local Ollama/OpenRouter reasoning models. Nothing upstream of the app
//! removes it, so the whole scratchpad was typed into the user's document ahead
//! of the one line they actually dictated.
//!
//! Asking each provider to hide its reasoning is the cheaper fix where it is
//! available (see `openai::reasoning_params`), but it is per-provider and
//! per-model, and Groq rejects a parameter a model does not know: a request that
//! carries `reasoning_effort` to the wrong model fails outright. This filter is
//! therefore the layer that has to hold for every provider, including ones added
//! later.
//!
//! It has to work on the *stream*, not just the final string: chunks are
//! forwarded to the frontend as they arrive, so by the time a response is
//! complete the scratchpad has already been drawn. That means tolerating a tag
//! split across two chunks — `<thi` / `nk>` is one arrival away from being
//! ordinary text, so a partial tag is held back until the next chunk resolves it.

/// Longest tag we recognise; bounds how much text can be held back at a chunk
/// boundary waiting to find out whether it is a tag.
const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

/// Incremental `<think>` stripper. Feed it content deltas with [`push`], then
/// call [`finish`] once the stream ends.
///
/// [`push`]: ThinkFilter::push
/// [`finish`]: ThinkFilter::finish
#[derive(Debug, Default)]
pub struct ThinkFilter {
    /// Currently between an opening tag and its close.
    inside: bool,
    /// Text that arrived but cannot be classified yet: either we are mid-tag, or
    /// the chunk ended on something that could still grow into one.
    pending: String,
    /// A reasoning block was seen at some point. Lets the caller tell "the model
    /// produced nothing" apart from "the model produced only reasoning", which
    /// are different failures.
    saw_reasoning: bool,
    /// The stream ended inside an unterminated block — the answer was cut off
    /// mid-scratchpad, typically by `max_tokens`.
    truncated_in_reasoning: bool,
}

impl ThinkFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// True once a `<think>` block has been seen and dropped.
    pub fn saw_reasoning(&self) -> bool {
        self.saw_reasoning
    }

    /// True when the stream ended while still inside a reasoning block, meaning
    /// the visible answer was never emitted.
    pub fn truncated_in_reasoning(&self) -> bool {
        self.truncated_in_reasoning
    }

    /// Feed one content delta; returns the portion safe to show the user now.
    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        let mut out = String::new();

        loop {
            if self.inside {
                match self.pending.find(CLOSE) {
                    Some(i) => {
                        self.pending.drain(..i + CLOSE.len());
                        self.inside = false;
                        self.saw_reasoning = true;
                    }
                    None => {
                        // Everything buffered is reasoning except a possible
                        // partial closing tag at the very end.
                        let hold = partial_tag_suffix(&self.pending);
                        let drop_to = self.pending.len() - hold;
                        self.pending.drain(..drop_to);
                        break;
                    }
                }
            } else {
                // A close with no matching open means the model started in
                // reasoning mode without announcing it — some providers strip the
                // opening tag but leave the closing one. Treat everything before
                // it as scratchpad rather than typing it out.
                let open_at = self.pending.find(OPEN);
                let close_at = self.pending.find(CLOSE);
                let opens_first = match (open_at, close_at) {
                    (Some(o), Some(c)) => o < c,
                    (Some(_), None) => true,
                    (None, _) => false,
                };

                if let (true, Some(o)) = (opens_first, open_at) {
                    out.push_str(&self.pending[..o]);
                    self.pending.drain(..o + OPEN.len());
                    self.inside = true;
                } else if let Some(c) = close_at {
                    self.pending.drain(..c + CLOSE.len());
                    self.saw_reasoning = true;
                } else {
                    let hold = partial_tag_suffix(&self.pending);
                    let emit_to = self.pending.len() - hold;
                    out.push_str(&self.pending[..emit_to]);
                    self.pending.drain(..emit_to);
                    break;
                }
            }
        }

        out
    }

    /// Flush whatever is left once the stream is done. Anything still buffered
    /// inside a reasoning block is dropped, and the block is recorded as
    /// truncated so the caller can treat the response as a failure rather than
    /// typing an empty string over the user's text.
    pub fn finish(&mut self) -> String {
        if self.inside {
            self.saw_reasoning = true;
            self.truncated_in_reasoning = true;
            self.pending.clear();
            return String::new();
        }
        std::mem::take(&mut self.pending)
    }
}

/// Length of the longest suffix of `s` that could still grow into `<think>` or
/// `</think>`. Held back rather than emitted, so a tag split across two chunks
/// is not typed out as literal text.
fn partial_tag_suffix(s: &str) -> usize {
    let max = CLOSE.len() - 1;
    for k in (1..=max.min(s.len())).rev() {
        let start = s.len() - k;
        // Tag characters are ASCII, so a candidate that starts mid-codepoint
        // cannot be one; skipping keeps the slice below panic-free.
        if !s.is_char_boundary(start) {
            continue;
        }
        let tail = &s[start..];
        if OPEN.starts_with(tail) || CLOSE.starts_with(tail) {
            return k;
        }
    }
    0
}

/// One-shot convenience for the non-streaming path.
pub fn strip(text: &str) -> String {
    let mut f = ThinkFilter::new();
    let mut out = f.push(text);
    out.push_str(&f.finish());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds the text one chunk at a time and returns everything forwarded.
    fn stream(chunks: &[&str]) -> (String, ThinkFilter) {
        let mut f = ThinkFilter::new();
        let mut out = String::new();
        for c in chunks {
            out.push_str(&f.push(c));
        }
        out.push_str(&f.finish());
        (out, f)
    }

    #[test]
    fn passes_plain_text_through_untouched() {
        let (out, f) = stream(&["One, two, three."]);
        assert_eq!(out, "One, two, three.");
        assert!(!f.saw_reasoning());
    }

    /// The reported failure: Groq's `qwen/qwen3.6-27b` defaults to raw reasoning,
    /// so the whole scratchpad arrived in `content` ahead of the answer and was
    /// typed into the user's document.
    #[test]
    fn drops_a_reasoning_block_and_keeps_the_answer() {
        let (out, f) = stream(&[
            "\n<think>\nThe user said \"One, two, three\". Rule 5 says format \
             enumerations as a list. Wait, should I add a period? Let me \
             reconsider.\n</think>\n\n1. One\n2. Two\n3. Three",
        ]);
        assert_eq!(out.trim(), "1. One\n2. Two\n3. Three");
        assert!(!out.contains("<think>"));
        assert!(!out.contains("reconsider"));
        assert!(f.saw_reasoning());
        assert!(!f.truncated_in_reasoning());
    }

    /// Chunks arrive at arbitrary boundaries, so a tag is routinely split. If a
    /// partial tag were forwarded it would be typed as literal text and the rest
    /// of the scratchpad would follow it.
    #[test]
    fn handles_a_tag_split_across_chunks() {
        let (out, _) = stream(&[
            "Hello <",
            "thi",
            "nk>secret",
            " notes</",
            "thi",
            "nk> world",
        ]);
        assert_eq!(out, "Hello  world");
    }

    #[test]
    fn handles_every_single_character_split() {
        let src = "before<think>hidden</think>after";
        let chunks: Vec<String> = src.chars().map(|c| c.to_string()).collect();
        let refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
        let (out, f) = stream(&refs);
        assert_eq!(out, "beforeafter");
        assert!(f.saw_reasoning());
    }

    /// A `<` that never becomes a tag must still reach the user — held-back text
    /// is a delay, never a deletion.
    #[test]
    fn releases_a_partial_tag_that_turns_out_to_be_ordinary_text() {
        let (out, f) = stream(&["compare a <", " b and c <thi", "s too"]);
        assert_eq!(out, "compare a < b and c <this too");
        assert!(!f.saw_reasoning());
    }

    #[test]
    fn releases_a_dangling_partial_tag_at_end_of_stream() {
        let (out, _) = stream(&["done <thin"]);
        assert_eq!(out, "done <thin");
    }

    /// `max_tokens` cut the response mid-scratchpad, so there is no answer at
    /// all. Reporting that is what lets the pipeline fall back to the raw
    /// transcript instead of typing an empty string.
    #[test]
    fn reports_truncation_when_the_block_never_closes() {
        let (out, f) = stream(&["<think>still deliberating and then the budget ran out"]);
        assert_eq!(out, "");
        assert!(f.saw_reasoning());
        assert!(f.truncated_in_reasoning());
    }

    /// Some providers strip the opening tag but leave the close, which would
    /// otherwise let the entire scratchpad through.
    #[test]
    fn drops_reasoning_when_only_the_closing_tag_is_present() {
        let (out, f) = stream(&["deliberating out loud</think>The answer."]);
        assert_eq!(out, "The answer.");
        assert!(f.saw_reasoning());
    }

    #[test]
    fn drops_several_blocks_in_one_response() {
        let (out, _) = stream(&["a<think>x</think>b<think>y</think>c"]);
        assert_eq!(out, "abc");
    }

    /// Held-back suffixes are sliced by byte offset, so multi-byte text must not
    /// be cut mid-codepoint.
    #[test]
    fn does_not_panic_on_multibyte_text() {
        let (out, _) = stream(&["Привет, мир! <", "think>размышления</think> Готово. 日本語"]);
        assert_eq!(out, "Привет, мир!  Готово. 日本語");
    }

    #[test]
    fn strip_handles_the_non_streaming_path() {
        assert_eq!(strip("<think>reasoning</think>Answer."), "Answer.");
        assert_eq!(strip("Answer."), "Answer.");
    }
}
