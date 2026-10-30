//! The few things the window needs from Windows directly: message boxes, the
//! failure beep, opening a link, and administrator rights.

#[cfg(windows)]
mod imp {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONERROR, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_YESNO,
        MESSAGEBOX_STYLE, SW_SHOWNORMAL,
    };

    fn owner(hwnd: Option<isize>) -> Option<HWND> {
        hwnd.map(|h| HWND(h as *mut core::ffi::c_void))
    }

    fn show(hwnd: Option<isize>, caption: &str, text: &str, style: MESSAGEBOX_STYLE) -> bool {
        // SAFETY: plain strings; the owner window, when given, is ours.
        let result = unsafe {
            MessageBoxW(
                owner(hwnd),
                &HSTRING::from(text),
                &HSTRING::from(caption),
                style,
            )
        };
        result == IDYES
    }

    pub fn warn(hwnd: Option<isize>, caption: &str, text: &str) {
        show(hwnd, caption, text, MB_OK | MB_ICONWARNING);
    }

    pub fn error(hwnd: Option<isize>, caption: &str, text: &str) {
        show(hwnd, caption, text, MB_OK | MB_ICONERROR);
    }

    pub fn ask_yes_no(hwnd: Option<isize>, caption: &str, text: &str) -> bool {
        show(hwnd, caption, text, MB_YESNO | MB_ICONQUESTION)
    }

    pub fn bell() {
        // SAFETY: no pointers.
        let _ = unsafe { MessageBeep(MB_OK) };
    }

    pub fn open_url(url: &str) {
        // SAFETY: plain strings.
        unsafe {
            ShellExecuteW(
                None,
                &HSTRING::from("open"),
                &HSTRING::from(url),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
        }
    }

    pub fn is_admin() -> bool {
        // SAFETY: no pointers.
        unsafe { IsUserAnAdmin() }.as_bool()
    }

    /// Start this executable again through UAC. True when the request was
    /// accepted by the shell (the user may still decline the prompt).
    pub fn relaunch_as_admin() -> bool {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        // SAFETY: plain strings.
        let result = unsafe {
            ShellExecuteW(
                None,
                &HSTRING::from("runas"),
                &HSTRING::from(exe.as_os_str()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        result.0 as isize > 32
    }

    /// Give a GUI-subsystem process the console it was started from, so
    /// `--selftest` can print. Fails harmlessly when there is none.
    pub fn attach_parent_console() {
        use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        // SAFETY: no pointers.
        let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn warn(_: Option<isize>, _: &str, text: &str) {
        eprintln!("{text}");
    }
    pub fn error(_: Option<isize>, _: &str, text: &str) {
        eprintln!("{text}");
    }
    pub fn ask_yes_no(_: Option<isize>, _: &str, _: &str) -> bool {
        false
    }
    pub fn bell() {}
    pub fn open_url(_: &str) {}
    pub fn is_admin() -> bool {
        false
    }
    pub fn relaunch_as_admin() -> bool {
        false
    }
    pub fn attach_parent_console() {}
}

pub use imp::*;
