//! Dictated text in log lines.
//!
//! With `VOXTYPE_REDACT_TEXT=1` in the environment, every log line that would
//! quote what was dictated, transcribed or post-processed shows only its length,
//! so a log can be shared without the words in it. The programs the daemon
//! starts (transcription workers, the post-process command) inherit the
//! variable. `voxtype --help` names it, so a caller can tell whether this build
//! honours it.

use std::fmt;
use std::sync::OnceLock;

/// The environment variable that turns redaction on (any value but empty or `0`).
pub const ENV: &str = "VOXTYPE_REDACT_TEXT";

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var(ENV).is_ok_and(|value| !value.is_empty() && value != "0"))
}

/// Text for a log line: quoted as `{:?}` would, or `[N chars]` when redaction is on.
pub fn text(value: &str) -> Text<'_> {
    Text::new(value, enabled(), None)
}

/// Like [`text`], but quotes at most `limit` characters, followed by `...`.
pub fn preview(value: &str, limit: usize) -> Text<'_> {
    Text::new(value, enabled(), Some(limit))
}

/// Optional text, such as the previous dictation passed as context.
pub fn optional(value: Option<&str>) -> Optional<'_> {
    Optional(value.map(text))
}

pub struct Text<'a> {
    value: &'a str,
    redact: bool,
    limit: Option<usize>,
}

impl<'a> Text<'a> {
    fn new(value: &'a str, redact: bool, limit: Option<usize>) -> Self {
        Self {
            value,
            redact,
            limit,
        }
    }
}

impl fmt::Display for Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.value.chars().count();
        if self.redact {
            return write!(f, "[{} chars]", count);
        }
        match self.limit {
            Some(limit) if count > limit => {
                let shown: String = self.value.chars().take(limit).collect();
                write!(f, "{:?}", format!("{}...", shown))
            }
            _ => write!(f, "{:?}", self.value),
        }
    }
}

pub struct Optional<'a>(Option<Text<'a>>);

impl fmt::Display for Optional<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(text) => write!(f, "Some({})", text),
            None => write!(f, "None"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_text_is_quoted_as_before() {
        assert_eq!(
            Text::new("say \"hi\"", false, None).to_string(),
            "\"say \\\"hi\\\"\""
        );
        assert_eq!(
            Text::new("abcdef", false, Some(3)).to_string(),
            "\"abc...\""
        );
        assert_eq!(Text::new("abc", false, Some(3)).to_string(), "\"abc\"");
    }

    #[test]
    fn redacted_text_shows_only_its_length() {
        assert_eq!(Text::new("xin chào", true, None).to_string(), "[8 chars]");
        assert_eq!(Text::new("abcdef", true, Some(3)).to_string(), "[6 chars]");
        assert_eq!(
            Optional(Some(Text::new("earlier", true, None))).to_string(),
            "Some([7 chars])"
        );
        assert_eq!(Optional(None).to_string(), "None");
    }
}
