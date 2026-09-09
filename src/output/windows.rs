//! Native Windows text and clipboard output.

use std::borrow::Cow;
use std::mem::size_of;
use std::time::Duration;

use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};

use super::TextOutput;
use crate::error::OutputError;

fn with_append<'a>(text: &'a str, append: &Option<String>) -> Cow<'a, str> {
    match append {
        Some(suffix) => Cow::Owned(format!("{text}{suffix}")),
        None => Cow::Borrowed(text),
    }
}

/// Store UTF-16 text in `CF_UNICODETEXT`. Clipboard contention is common, so
/// opening it uses a short bounded retry.
pub fn set_clipboard_text(text: &str) -> Result<(), OutputError> {
    let mut opened = false;
    for attempt in 0..10 {
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            opened = true;
            break;
        }
        if attempt != 9 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if !opened {
        return Err(OutputError::InjectionFailed(format!(
            "could not open the Windows clipboard: {}",
            std::io::Error::last_os_error()
        )));
    }

    let result = (|| {
        if unsafe { EmptyClipboard() } == 0 {
            return Err(OutputError::InjectionFailed(format!(
                "could not clear the Windows clipboard: {}",
                std::io::Error::last_os_error()
            )));
        }

        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let byte_len = utf16.len() * size_of::<u16>();
        let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) };
        if memory.is_null() {
            return Err(OutputError::InjectionFailed(
                "could not allocate clipboard memory".to_string(),
            ));
        }

        let destination = unsafe { GlobalLock(memory) } as *mut u16;
        if destination.is_null() {
            unsafe { GlobalFree(memory) };
            return Err(OutputError::InjectionFailed(
                "could not lock clipboard memory".to_string(),
            ));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(utf16.as_ptr(), destination, utf16.len());
            GlobalUnlock(memory);
        }

        if unsafe { SetClipboardData(CF_UNICODETEXT as u32, memory as HANDLE) }.is_null() {
            unsafe { GlobalFree(memory) };
            return Err(OutputError::InjectionFailed(format!(
                "could not set Windows clipboard text: {}",
                std::io::Error::last_os_error()
            )));
        }
        // The system owns `memory` after SetClipboardData succeeds.
        Ok(())
    })();

    unsafe { CloseClipboard() };
    result
}

fn keyboard_input(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_inputs(inputs: &[INPUT]) -> Result<(), OutputError> {
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        Err(OutputError::InjectionFailed(format!(
            "Windows accepted {sent} of {} keyboard events: {}",
            inputs.len(),
            std::io::Error::last_os_error()
        )))
    }
}

struct UnicodeInputFailure {
    error: OutputError,
    partial: bool,
}

fn send_unicode(text: &str, delay_ms: u32) -> Result<(), UnicodeInputFailure> {
    if delay_ms == 0 {
        let mut inputs = Vec::with_capacity(text.encode_utf16().count() * 2);
        for unit in text.encode_utf16() {
            inputs.push(keyboard_input(0, unit, KEYEVENTF_UNICODE));
            inputs.push(keyboard_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                size_of::<INPUT>() as i32,
            )
        };
        return if sent == inputs.len() as u32 {
            Ok(())
        } else {
            Err(UnicodeInputFailure {
                error: OutputError::InjectionFailed(format!(
                    "Windows accepted {sent} of {} Unicode keyboard events: {}",
                    inputs.len(),
                    std::io::Error::last_os_error()
                )),
                partial: sent > 0,
            })
        };
    }
    let mut emitted = false;
    for unit in text.encode_utf16() {
        let inputs = [
            keyboard_input(0, unit, KEYEVENTF_UNICODE),
            keyboard_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
        ];
        let sent = unsafe { SendInput(2, inputs.as_ptr(), size_of::<INPUT>() as i32) };
        if sent != 2 {
            return Err(UnicodeInputFailure {
                error: OutputError::InjectionFailed(format!(
                    "Windows accepted {sent} of 2 Unicode keyboard events: {}",
                    std::io::Error::last_os_error()
                )),
                partial: emitted || sent > 0,
            });
        }
        emitted = true;
        if delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms as u64));
        }
    }
    Ok(())
}

fn send_key(vk: u16) -> Result<(), OutputError> {
    send_inputs(&[
        keyboard_input(vk, 0, 0),
        keyboard_input(vk, 0, KEYEVENTF_KEYUP),
    ])
}

pub fn send_backspaces(count: usize) -> Result<(), OutputError> {
    for _ in 0..count {
        send_key(windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_BACK)?;
    }
    Ok(())
}

fn send_ctrl_v() -> Result<(), OutputError> {
    send_inputs(&[
        keyboard_input(VK_CONTROL, 0, 0),
        keyboard_input(b'V' as u16, 0, 0),
        keyboard_input(b'V' as u16, 0, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, 0, KEYEVENTF_KEYUP),
    ])
}

fn modifiers_down() -> bool {
    const MODIFIERS: [u16; 12] = [
        VK_SHIFT,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
        0,
    ];
    MODIFIERS[..11]
        .iter()
        .any(|vk| unsafe { GetAsyncKeyState(*vk as i32) } < 0)
}

pub async fn wait_for_modifiers_release(timeout: Duration) -> bool {
    let started = tokio::time::Instant::now();
    while modifiers_down() {
        if started.elapsed() >= timeout {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    true
}

pub struct WindowsTypingOutput {
    auto_submit: bool,
    append_text: Option<String>,
    type_delay_ms: u32,
    pre_type_delay_ms: u32,
}

impl WindowsTypingOutput {
    pub fn new(
        auto_submit: bool,
        append_text: Option<String>,
        type_delay_ms: u32,
        pre_type_delay_ms: u32,
    ) -> Self {
        Self {
            auto_submit,
            append_text,
            type_delay_ms,
            pre_type_delay_ms,
        }
    }
}

#[async_trait::async_trait]
impl TextOutput for WindowsTypingOutput {
    async fn output(&self, text: &str) -> Result<(), OutputError> {
        let text = with_append(text, &self.append_text);
        if text.is_empty() {
            return Ok(());
        }
        if self.pre_type_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.pre_type_delay_ms as u64)).await;
        }
        if let Err(failure) = send_unicode(&text, self.type_delay_ms) {
            set_clipboard_text(&text)?;
            if !failure.partial && send_ctrl_v().is_ok() {
                if self.auto_submit {
                    let _ = send_key(VK_RETURN);
                }
                tracing::debug!(
                    "Direct Windows typing failed ({}); used clipboard paste",
                    failure.error
                );
                return Ok(());
            }
            crate::notification::send(
                "VoxType",
                "Windows blocked keyboard input. The transcription remains on the clipboard.",
            )
            .await;
            tracing::warn!("Direct Windows typing failed: {}", failure.error);
            return Ok(());
        }
        if self.auto_submit {
            if let Err(error) = send_key(VK_RETURN) {
                tracing::warn!("Text was typed, but Windows blocked automatic submit: {error}");
            }
        }
        Ok(())
    }

    async fn is_available(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "windows-sendinput"
    }
}

pub struct WindowsPasteOutput {
    auto_submit: bool,
    append_text: Option<String>,
    pre_type_delay_ms: u32,
}

impl WindowsPasteOutput {
    pub fn new(auto_submit: bool, append_text: Option<String>, pre_type_delay_ms: u32) -> Self {
        Self {
            auto_submit,
            append_text,
            pre_type_delay_ms,
        }
    }
}

#[async_trait::async_trait]
impl TextOutput for WindowsPasteOutput {
    async fn output(&self, text: &str) -> Result<(), OutputError> {
        let text = with_append(text, &self.append_text);
        set_clipboard_text(&text)?;
        if self.pre_type_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.pre_type_delay_ms as u64)).await;
        }
        if let Err(error) = send_ctrl_v() {
            crate::notification::send(
                "VoxType",
                "Windows blocked paste input. The transcription remains on the clipboard.",
            )
            .await;
            return Err(OutputError::CtrlVFailed(error.to_string()));
        }
        if self.auto_submit {
            send_key(VK_RETURN)?;
        }
        Ok(())
    }

    async fn is_available(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "paste (Windows clipboard + Ctrl+V)"
    }
}

pub struct WindowsClipboardOutput {
    append_text: Option<String>,
}

impl WindowsClipboardOutput {
    pub fn new(append_text: Option<String>) -> Self {
        Self { append_text }
    }
}

#[async_trait::async_trait]
impl TextOutput for WindowsClipboardOutput {
    async fn output(&self, text: &str) -> Result<(), OutputError> {
        set_clipboard_text(&with_append(text, &self.append_text))
    }

    async fn is_available(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "clipboard (Windows CF_UNICODETEXT)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_is_applied_once() {
        assert_eq!(with_append("hello", &Some("!".into())), "hello!");
        assert_eq!(with_append("hello", &None), "hello");
    }
}
