//! Native Windows global hotkeys using a low-level keyboard hook.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::mpsc;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE,
    VK_HOME, VK_INSERT, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_NEXT,
    VK_PAUSE, VK_PRIOR, VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SCROLL,
    VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

use crate::config::HotkeyConfig;
use crate::error::HotkeyError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed {
        model_override: Option<String>,
        profile_override: Option<String>,
    },
    Released,
    Cancel,
}

pub trait HotkeyListener: Send {
    fn start(&mut self) -> Result<mpsc::UnboundedReceiver<HotkeyEvent>, HotkeyError>;
    fn stop(&mut self) -> Result<(), HotkeyError>;
}

struct HookState {
    sender: mpsc::UnboundedSender<HotkeyEvent>,
    target: u16,
    cancel: Option<u16>,
    required_modifiers: Vec<u16>,
    model_modifier: Option<(u16, String)>,
    profile_modifiers: Vec<(u16, String)>,
    target_down: bool,
    target_active: bool,
    cancel_down: bool,
}

static HOOK_STATE: OnceLock<Mutex<Option<HookState>>> = OnceLock::new();

fn hook_state() -> &'static Mutex<Option<HookState>> {
    HOOK_STATE.get_or_init(|| Mutex::new(None))
}

fn is_key_down(vk: u16) -> bool {
    (unsafe { GetAsyncKeyState(vk as i32) }) < 0
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if event.flags & LLKHF_INJECTED != 0 {
            return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
        }

        let is_down = wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;
        let is_up = wparam == WM_KEYUP as usize || wparam == WM_SYSKEYUP as usize;
        if let Ok(mut guard) = hook_state().lock() {
            if let Some(state) = guard.as_mut() {
                let vk = event.vkCode as u16;
                if Some(vk) == state.cancel {
                    if is_down && !state.cancel_down {
                        state.cancel_down = true;
                        let _ = state.sender.send(HotkeyEvent::Cancel);
                    } else if is_up {
                        state.cancel_down = false;
                    }
                } else if vk == state.target {
                    if is_down && !state.target_down {
                        state.target_down = true;
                        if state.required_modifiers.iter().all(|key| is_key_down(*key)) {
                            state.target_active = true;
                            let model_override = state
                                .model_modifier
                                .as_ref()
                                .and_then(|(key, model)| is_key_down(*key).then(|| model.clone()));
                            let profile_override =
                                state.profile_modifiers.iter().find_map(|(key, profile)| {
                                    is_key_down(*key).then(|| profile.clone())
                                });
                            let _ = state.sender.send(HotkeyEvent::Pressed {
                                model_override,
                                profile_override,
                            });
                        }
                    } else if is_up && state.target_down {
                        state.target_down = false;
                        if state.target_active {
                            state.target_active = false;
                            let _ = state.sender.send(HotkeyEvent::Released);
                        }
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn parse_key_name(name: &str) -> Option<u16> {
    let upper = name.trim().to_ascii_uppercase();
    if upper.len() == 1 {
        let c = upper.as_bytes()[0];
        if c.is_ascii_alphanumeric() {
            return Some(c as u16);
        }
    }
    if let Some(number) = upper.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
        if (1..=24).contains(&number) {
            return Some(0x70 + number - 1);
        }
    }
    Some(match upper.as_str() {
        "ESC" | "ESCAPE" => VK_ESCAPE,
        "SPACE" => VK_SPACE,
        "TAB" => VK_TAB,
        "ENTER" | "RETURN" => VK_RETURN,
        "BACKSPACE" => VK_BACK,
        "DELETE" => VK_DELETE,
        "INSERT" => VK_INSERT,
        "HOME" => VK_HOME,
        "END" => VK_END,
        "PAGEUP" => VK_PRIOR,
        "PAGEDOWN" => VK_NEXT,
        "UP" | "UPARROW" => VK_UP,
        "DOWN" | "DOWNARROW" => VK_DOWN,
        "LEFT" | "LEFTARROW" => VK_LEFT,
        "RIGHT" | "RIGHTARROW" => VK_RIGHT,
        "CAPSLOCK" => VK_CAPITAL,
        "SCROLLLOCK" => VK_SCROLL,
        "PAUSE" => VK_PAUSE,
        "SHIFT" => VK_SHIFT,
        "LEFTSHIFT" => VK_LSHIFT,
        "RIGHTSHIFT" => VK_RSHIFT,
        "CTRL" | "CONTROL" => VK_CONTROL,
        "LEFTCTRL" | "LEFTCONTROL" => VK_LCONTROL,
        "RIGHTCTRL" | "RIGHTCONTROL" => VK_RCONTROL,
        "ALT" | "OPTION" => VK_MENU,
        "LEFTALT" | "LEFTOPTION" => VK_LMENU,
        "RIGHTALT" | "RIGHTOPTION" => VK_RMENU,
        "WIN" | "META" | "SUPER" | "LEFTWIN" | "LEFTMETA" => VK_LWIN,
        "RIGHTWIN" | "RIGHTMETA" => VK_RWIN,
        _ => return None,
    })
}

pub struct WindowsHotkeyListener {
    config: HotkeyConfig,
    secondary_model: Option<String>,
    running: Arc<AtomicBool>,
    thread_id: Option<u32>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl WindowsHotkeyListener {
    fn new(config: &HotkeyConfig, secondary_model: Option<String>) -> Result<Self, HotkeyError> {
        if config.key.trim().is_empty() {
            return Err(HotkeyError::Windows(
                "hotkey.enabled=true requires hotkey.key to name a key".to_string(),
            ));
        }
        parse_key_name(&config.key).ok_or_else(|| HotkeyError::UnknownKey(config.key.clone()))?;
        Ok(Self {
            config: config.clone(),
            secondary_model,
            running: Arc::new(AtomicBool::new(false)),
            thread_id: None,
            thread: None,
        })
    }
}

impl HotkeyListener for WindowsHotkeyListener {
    fn start(&mut self) -> Result<mpsc::UnboundedReceiver<HotkeyEvent>, HotkeyError> {
        use windows_sys::Win32::System::Threading::GetCurrentThreadId;

        let target = parse_key_name(&self.config.key)
            .ok_or_else(|| HotkeyError::UnknownKey(self.config.key.clone()))?;
        let cancel = self
            .config
            .cancel_key
            .as_deref()
            .map(|key| parse_key_name(key).ok_or_else(|| HotkeyError::UnknownKey(key.into())))
            .transpose()?;
        let required_modifiers = self
            .config
            .modifiers
            .iter()
            .map(|key| parse_key_name(key).ok_or_else(|| HotkeyError::UnknownKey(key.clone())))
            .collect::<Result<Vec<_>, _>>()?;
        let model_modifier = match (
            self.config.model_modifier.as_deref(),
            self.secondary_model.as_ref(),
        ) {
            (Some(key), Some(model)) => Some((
                parse_key_name(key).ok_or_else(|| HotkeyError::UnknownKey(key.into()))?,
                model.clone(),
            )),
            _ => None,
        };
        let profile_modifiers = self
            .config
            .profile_modifiers
            .iter()
            .map(|(key, profile)| {
                parse_key_name(key)
                    .map(|vk| (vk, profile.clone()))
                    .ok_or_else(|| HotkeyError::UnknownKey(key.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let (tx, rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let running = self.running.clone();
        running.store(true, Ordering::Release);
        let thread = std::thread::spawn(move || {
            let thread_id = unsafe { GetCurrentThreadId() };
            let mut message: MSG = unsafe { std::mem::zeroed() };
            // Ensure this thread owns a message queue before publishing its
            // ID so stop() can always post its shutdown request.
            unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };
            let hook = unsafe {
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), std::ptr::null_mut(), 0)
            };
            if hook.is_null() {
                let _ = ready_tx.send(Err(std::io::Error::last_os_error().to_string()));
                running.store(false, Ordering::Release);
                return;
            }
            *hook_state().lock().expect("Windows hotkey state") = Some(HookState {
                sender: tx,
                target,
                cancel,
                required_modifiers,
                model_modifier,
                profile_modifiers,
                target_down: false,
                target_active: false,
                cancel_down: false,
            });
            let _ = ready_tx.send(Ok(thread_id));

            while running.load(Ordering::Acquire)
                && unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0
            {
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }

            *hook_state().lock().expect("Windows hotkey state") = None;
            unsafe { UnhookWindowsHookEx(hook) };
        });

        match ready_rx.recv() {
            Ok(Ok(thread_id)) => {
                self.thread_id = Some(thread_id);
                self.thread = Some(thread);
                Ok(rx)
            }
            Ok(Err(message)) => {
                let _ = thread.join();
                Err(HotkeyError::Windows(message))
            }
            Err(error) => {
                let _ = thread.join();
                Err(HotkeyError::Windows(error.to_string()))
            }
        }
    }

    fn stop(&mut self) -> Result<(), HotkeyError> {
        self.running.store(false, Ordering::Release);
        if let Some(thread_id) = self.thread_id.take() {
            if unsafe { PostThreadMessageW(thread_id, WM_QUIT, 0, 0) } == 0 {
                tracing::debug!(
                    "Could not wake Windows hotkey thread: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        Ok(())
    }
}

pub fn create_listener(
    config: &HotkeyConfig,
    secondary_model: Option<String>,
) -> Result<Box<dyn HotkeyListener>, HotkeyError> {
    Ok(Box::new(WindowsHotkeyListener::new(
        config,
        secondary_model,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_key_names() {
        assert_eq!(parse_key_name("F24"), Some(0x87));
        assert_eq!(parse_key_name("scrolllock"), Some(VK_SCROLL));
        assert_eq!(parse_key_name("A"), Some(b'A' as u16));
        assert_eq!(parse_key_name("not-a-key"), None);
    }
}
