//! Finding and starting OBS (ObsRecorderAdapter.md Internal Logic A and `open_obs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The operating systems the adapter knows; whether the computer's sound can be recorded depends
/// on it (Internal Logic B4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    MacOs,
    Windows,
    Other,
}

impl Platform {
    pub(crate) fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Other
        }
    }
}

/// Finds the installed program (the bundle on macOS, `obs64.exe` on Windows) at each call, so an
/// OBS installed while Ordin runs is found.
pub(crate) type Detect = Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>;
/// Starts the program `detect` found.
pub(crate) type Launch = Arc<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync>;

/// The computer the adapter runs on; the tests give their own.
#[derive(Clone)]
pub(crate) struct Host {
    pub platform: Platform,
    pub detect: Detect,
    pub launch: Launch,
}

impl Host {
    pub(crate) fn current() -> Host {
        Host {
            platform: Platform::current(),
            detect: Arc::new(installed_program),
            launch: Arc::new(launch_program),
        }
    }
}

/// A1: macOS `/Applications/OBS.app` or `~/Applications/OBS.app`; Windows the folder named by the
/// default value of `HKLM\SOFTWARE\OBS Studio`, else `%ProgramFiles%\obs-studio`, holding
/// `bin\64bit\obs64.exe`; other systems never.
fn installed_program() -> Option<PathBuf> {
    match Platform::current() {
        Platform::MacOs => {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            std::iter::once(PathBuf::from("/Applications/OBS.app"))
                .chain(home.map(|h| h.join("Applications").join("OBS.app")))
                .find(|bundle| bundle.is_dir())
        }
        Platform::Windows => {
            let program_files =
                std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("obs-studio"));
            registry_folder()
                .into_iter()
                .chain(program_files)
                .map(|folder| folder.join("bin").join("64bit").join("obs64.exe"))
                .find(|program| program.is_file())
        }
        Platform::Other => None,
    }
}

/// macOS: `open -a <bundle>`; Windows: `obs64.exe` with its own folder as the working directory,
/// which OBS needs to find its files.
fn launch_program(program: &Path) -> std::io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut open = std::process::Command::new("open");
        open.arg("-a").arg(program);
        open
    } else {
        let mut obs = std::process::Command::new(program);
        if let Some(folder) = program.parent() {
            obs.current_dir(folder);
        }
        obs
    };
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
}

#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) fn registry_folder() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    let key: Vec<u16> = "SOFTWARE\\OBS Studio\0".encode_utf16().collect();
    let mut bytes: u32 = 0;
    // SAFETY: `key` is a NUL-terminated UTF-16 string that outlives the call; a null value name
    // reads the key's default value; with a null buffer the call only writes the size to `bytes`.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS || bytes < 2 {
        return None;
    }
    let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
    let mut size = u32::try_from(buffer.len() * 2).ok()?;
    // SAFETY: `buffer` holds `size` bytes and outlives the call, which writes at most `size` bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    (length > 0).then(|| PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length])))
}

#[cfg(not(windows))]
fn registry_folder() -> Option<PathBuf> {
    None
}
