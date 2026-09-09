//! Small operating-system services shared by the daemon and CLI.

use std::path::Path;
#[cfg(target_os = "windows")]
use std::path::PathBuf;

#[cfg(unix)]
pub struct DaemonInstance {
    _lock: pidlock::Pidlock,
}

#[cfg(unix)]
impl DaemonInstance {
    pub fn acquire(path: &Path) -> Result<Self, String> {
        let path_string = path.to_string_lossy().into_owned();
        let mut lock = pidlock::Pidlock::new(&path_string);
        lock.acquire().map_err(|e| format!("{e:?}"))?;
        Ok(Self { _lock: lock })
    }
}

#[cfg(target_os = "windows")]
pub struct DaemonInstance {
    handle: windows_sys::Win32::Foundation::HANDLE,
    lock_path: PathBuf,
}

#[cfg(target_os = "windows")]
impl DaemonInstance {
    pub fn acquire(path: &Path) -> Result<Self, String> {
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let user = std::env::var("USERNAME").unwrap_or_else(|_| "unknown".to_string());
        let domain = std::env::var("USERDOMAIN").unwrap_or_default();
        let identity: String = format!("{domain}_{user}")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let name: Vec<u16> = format!("Local\\VoxTypeDaemon_{identity}\0")
            .encode_utf16()
            .collect();

        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return Err("another VoxType daemon is already running for this user".to_string());
        }

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                unsafe { CloseHandle(handle) };
                return Err(e.to_string());
            }
        }
        if let Err(e) = std::fs::write(path, std::process::id().to_string()) {
            unsafe { CloseHandle(handle) };
            return Err(e.to_string());
        }

        Ok(Self {
            handle,
            lock_path: path.to_path_buf(),
        })
    }
}

#[cfg(target_os = "windows")]
impl Drop for DaemonInstance {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;

        let _ = std::fs::remove_file(&self.lock_path);
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

#[cfg(target_os = "windows")]
pub fn process_is_running(pid: i32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid <= 1 {
        return false;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32) };
    if handle.is_null() {
        return false;
    }
    let mut exit_code = 0;
    let ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) } != 0;
    unsafe { CloseHandle(handle) };
    ok && exit_code == STILL_ACTIVE as u32
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_live() {
        assert!(process_is_running(std::process::id() as i32));
        assert!(!process_is_running(0));
        assert!(!process_is_running(-1));
    }

    #[test]
    fn daemon_mutex_rejects_a_second_instance_and_cleans_its_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("voxtype.lock");
        let first = DaemonInstance::acquire(&path).unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            std::process::id().to_string()
        );
        assert!(DaemonInstance::acquire(&path).is_err());

        drop(first);
        assert!(!path.exists());
    }
}
