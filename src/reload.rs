//! `voxtype reload`: apply config.toml to the running daemon without ending it.
//!
//! File-trigger IPC, like `record cancel` and the meeting commands, so it works
//! the same on every platform. The CLI writes a request id to `reload` under the
//! runtime directory. The daemon takes the request only while it is idle, so a
//! dictation in progress finishes first. It then loads the config file again
//! and answers in `reload_result` with the same id, followed by `ok <parts>` or
//! `error <message>`.
//!
//! The daemon builds everything that can fail (hotkey listener, model) before
//! it swaps anything, so a rejected config leaves the running settings intact.

use std::path::{Path, PathBuf};

use crate::config::Config;

/// What a successful reload rebuilt beyond the settings read fresh for each
/// dictation (microphone, output, text processing, cleanup command).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    /// A hotkey listener was started, stopped or replaced (key, mode, modifiers or
    /// enabled changed while the built-in hotkey was on before or after).
    pub hotkey: bool,
    /// The transcription model was reloaded (engine or model settings changed).
    pub model: bool,
}

impl Applied {
    fn encode(self) -> String {
        let mut parts = Vec::new();
        if self.hotkey {
            parts.push("hotkey");
        }
        if self.model {
            parts.push("model");
        }
        parts.join(",")
    }

    fn decode(text: &str) -> Self {
        let parts: Vec<&str> = text.split(',').map(str::trim).collect();
        Self {
            hotkey: parts.contains(&"hotkey"),
            model: parts.contains(&"model"),
        }
    }

    /// A one-line summary for `voxtype reload`.
    pub fn describe(self) -> &'static str {
        match (self.hotkey, self.model) {
            (false, false) => "Settings applied.",
            (true, false) => "Settings applied; the hotkey listener was restarted.",
            (false, true) => "Settings applied; the transcription model was reloaded.",
            (true, true) => {
                "Settings applied; the hotkey listener was restarted and the model reloaded."
            }
        }
    }
}

pub fn request_path(dir: &Path) -> PathBuf {
    dir.join("reload")
}

pub fn result_path(dir: &Path) -> PathBuf {
    dir.join("reload_result")
}

/// Ask the daemon to reload. Clears any earlier answer first.
pub fn request(dir: &Path, id: &str) -> std::io::Result<()> {
    let _ = std::fs::remove_file(result_path(dir));
    std::fs::write(request_path(dir), id)
}

/// Withdraw a request the daemon has not taken yet. Only removes our own.
pub fn withdraw(dir: &Path, id: &str) {
    let path = request_path(dir);
    if std::fs::read_to_string(&path).is_ok_and(|text| text.trim() == id) {
        let _ = std::fs::remove_file(path);
    }
}

/// Take a pending request (daemon side): its id, with the file removed.
///
/// A request file that cannot be read is still consumed and answered with an
/// empty id, so it cannot make every poll retry it.
pub fn take_request(dir: &Path) -> Option<String> {
    let path = request_path(dir);
    if !path.exists() {
        return None;
    }
    let id = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    Some(id.trim().to_string())
}

/// Answer a request (daemon side).
pub fn answer(dir: &Path, id: &str, outcome: &Result<Applied, String>) {
    let body = match outcome {
        Ok(applied) => format!("{id}\nok {}\n", applied.encode()),
        // One line: the CLI prints it as is.
        Err(message) => format!("{id}\nerror {}\n", message.replace(['\r', '\n'], " ")),
    };
    let path = result_path(dir);
    let partial = path.with_extension("partial");
    // Written aside and renamed, so the CLI never reads half an answer.
    if std::fs::write(&partial, body).is_ok() && std::fs::rename(&partial, &path).is_err() {
        let _ = std::fs::remove_file(&partial);
    }
}

/// The daemon's answer to request `id`, once it has one (CLI side).
pub fn read_answer(dir: &Path, id: &str) -> Option<Result<Applied, String>> {
    let text = std::fs::read_to_string(result_path(dir)).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim() != id {
        return None;
    }
    let outcome = lines.next()?.trim();
    if let Some(parts) = outcome.strip_prefix("ok") {
        Some(Ok(Applied::decode(parts)))
    } else {
        Some(Err(outcome
            .strip_prefix("error")
            .unwrap_or(outcome)
            .trim()
            .to_string()))
    }
}

/// The settings that decide which transcription model is loaded. A reload
/// rebuilds the model only when these differ, so changing the microphone or
/// the hotkey keeps the loaded model warm.
pub fn model_settings(config: &Config) -> serde_json::Value {
    serde_json::json!({
        "engine": config.engine,
        "whisper": config.whisper,
        "parakeet": config.parakeet,
        "moonshine": config.moonshine,
        "sensevoice": config.sensevoice,
        "paraformer": config.paraformer,
        "dolphin": config.dolphin,
        "omnilingual": config.omnilingual,
        "cohere": config.cohere,
        "soniox": config.soniox,
    })
}

/// The settings the hotkey listener is built from.
pub fn hotkey_settings(config: &Config) -> serde_json::Value {
    serde_json::json!({
        "hotkey": config.hotkey,
        "secondary_model": config.whisper.secondary_model,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn answer_reaches_only_its_own_request() {
        let dir = TempDir::new().unwrap();
        request(dir.path(), "42").unwrap();
        assert_eq!(take_request(dir.path()).as_deref(), Some("42"));
        assert_eq!(take_request(dir.path()), None);
        assert_eq!(read_answer(dir.path(), "42"), None);

        let applied = Applied {
            hotkey: true,
            model: false,
        };
        answer(dir.path(), "42", &Ok(applied));
        assert_eq!(read_answer(dir.path(), "42"), Some(Ok(applied)));
        assert_eq!(read_answer(dir.path(), "41"), None);

        answer(dir.path(), "43", &Ok(Applied::default()));
        assert_eq!(read_answer(dir.path(), "43"), Some(Ok(Applied::default())));
    }

    #[test]
    fn errors_come_back_on_one_line() {
        let dir = TempDir::new().unwrap();
        answer(
            dir.path(),
            "7",
            &Err("Invalid config:\nunknown key MENU".into()),
        );
        assert_eq!(
            read_answer(dir.path(), "7"),
            Some(Err("Invalid config: unknown key MENU".into()))
        );
    }

    #[test]
    fn a_new_request_clears_the_last_answer_and_withdraw_keeps_others() {
        let dir = TempDir::new().unwrap();
        answer(dir.path(), "1", &Ok(Applied::default()));
        request(dir.path(), "2").unwrap();
        assert!(!result_path(dir.path()).exists());
        withdraw(dir.path(), "3");
        assert!(request_path(dir.path()).exists());
        withdraw(dir.path(), "2");
        assert!(!request_path(dir.path()).exists());
    }

    #[test]
    fn model_settings_ignore_microphone_and_hotkey() {
        let base = Config::default();
        let mut other = base.clone();
        other.audio.device = "USB microphone".into();
        other.hotkey.key = "RIGHTALT".into();
        assert_eq!(model_settings(&base), model_settings(&other));
        assert_ne!(hotkey_settings(&base), hotkey_settings(&other));

        other.whisper.model = "tiny.en".into();
        assert_ne!(model_settings(&base), model_settings(&other));
    }
}
