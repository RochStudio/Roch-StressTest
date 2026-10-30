//! Reading and filling in another program's window.
//!
//! The runner uses `windows_of` to tell a tool whose window was closed from
//! one that crashed. The rest -- finding a control by its label, typing into
//! it and reading the value back, pressing a button, noticing a message box --
//! was written for RAM Test Pro, which has no command line and no settings
//! file, and is kept for any tool that has to be driven through its window.
//!
//! That is a brittle thing to do, and it is done defensively:
//!
//!   * Controls are found by their *label*, not by position or index.
//!   * Every value written is read back. A box that did not take the value is
//!     reported rather than assumed.
//!   * Every message sent to another program's window goes through a timeout,
//!     so a hung window elsewhere on the desktop cannot hang this program.

/// One child window: what it is, what it says, and where it sits.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub hwnd: isize,
    pub class_name: String,
    pub text: String,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Control {
    pub fn middle(&self) -> f64 {
        (self.top + self.bottom) as f64 / 2.0
    }

    /// WinForms controls are named "WindowsForms10.Edit.app.0...."; a plain
    /// Win32 dialog just says "Edit". Both turn up.
    pub fn is_edit(&self) -> bool {
        self.class_name == "Edit" || self.class_name.contains(".Edit.")
    }

    pub fn is_button(&self) -> bool {
        self.class_name == "Button" || self.class_name.contains(".Button.")
    }

    /// The interesting part of a WinForms class name is the second piece; a
    /// plain Win32 class has only one.
    pub fn kind(&self) -> &str {
        self.class_name
            .split('.')
            .nth(1)
            .unwrap_or(&self.class_name)
    }
}

/// The text box on the same line as the label reading *label*.
///
/// Matched by vertical centre rather than by order, because "the box next to
/// Max Errors" survives a layout change and "the third box down" does not.
/// The box must also start to the right of the label. Ampersands are ignored
/// on both sides: a Win32 dialog carries the keyboard accelerator in the
/// caption.
pub fn box_beside<'a>(found: &'a [Control], label: &str, tolerance: f64) -> Option<&'a Control> {
    fn plain(text: &str) -> String {
        text.replace('&', "").trim().to_string()
    }
    let wanted = plain(label);
    let anchor = found.iter().find(|c| plain(&c.text) == wanted)?;
    found
        .iter()
        .filter(|c| {
            c.is_edit()
                && c.left >= anchor.left
                && (c.middle() - anchor.middle()).abs() <= tolerance
        })
        .min_by(|a, b| {
            let da = (a.middle() - anchor.middle()).abs();
            let db = (b.middle() - anchor.middle()).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// The default tolerance `box_beside` was written with.
pub const BOX_TOLERANCE: f64 = 14.0;

pub fn button<'a>(found: &'a [Control], label: &str) -> Option<&'a Control> {
    found
        .iter()
        .find(|c| c.is_button() && c.text.trim() == label)
}

#[cfg(windows)]
mod imp {
    use super::Control;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, EnumWindows, GetClassNameW, GetWindowRect, GetWindowThreadProcessId,
        IsWindowVisible, SendMessageTimeoutW, BM_CLICK, SMTO_ABORTIFHUNG, WM_GETTEXT,
        WM_GETTEXTLENGTH, WM_SETTEXT,
    };

    const MESSAGE_TIMEOUT_MS: u32 = 1000;

    fn hwnd(value: isize) -> HWND {
        HWND(value as *mut core::ffi::c_void)
    }

    fn send(target: HWND, message: u32, wparam: usize, lparam: isize) -> usize {
        let mut result = 0usize;
        // SAFETY: the timeout and SMTO_ABORTIFHUNG keep a hung target from
        // blocking us; lparam points at a live buffer whenever one is needed.
        let ok = unsafe {
            SendMessageTimeoutW(
                target,
                message,
                WPARAM(wparam),
                LPARAM(lparam),
                SMTO_ABORTIFHUNG,
                MESSAGE_TIMEOUT_MS,
                Some(&mut result),
            )
        };
        if ok.0 != 0 {
            result
        } else {
            0
        }
    }

    pub fn text_of(target: isize) -> String {
        let target = hwnd(target);
        let length = send(target, WM_GETTEXTLENGTH, 0, 0);
        if length == 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; length + 1];
        send(target, WM_GETTEXT, length + 1, buffer.as_mut_ptr() as isize);
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    pub fn pid_of(target: isize) -> u32 {
        let mut owner = 0u32;
        // SAFETY: owner is a valid out pointer.
        unsafe { GetWindowThreadProcessId(hwnd(target), Some(&mut owner)) };
        owner
    }

    pub fn is_visible(target: isize) -> bool {
        // SAFETY: no pointers.
        unsafe { IsWindowVisible(hwnd(target)) }.as_bool()
    }

    pub fn class_of(target: isize) -> String {
        let mut name = [0u16; 256];
        // SAFETY: the slice length is passed as the buffer size.
        let length = unsafe { GetClassNameW(hwnd(target), &mut name) };
        String::from_utf16_lossy(&name[..length.max(0) as usize])
    }

    unsafe extern "system" fn collect(window: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the &mut Vec<isize> passed by the caller below,
        // which outlives the enumeration.
        let list = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
        list.push(window.0 as isize);
        BOOL(1)
    }

    /// Every top-level window, visible or not.
    pub fn top_level() -> Vec<isize> {
        let mut found: Vec<isize> = Vec::new();
        // SAFETY: the callback only pushes into `found`.
        let _ = unsafe {
            EnumWindows(
                Some(collect),
                LPARAM(&mut found as *mut Vec<isize> as isize),
            )
        };
        found
    }

    pub fn children(parent: isize) -> Vec<isize> {
        let mut found: Vec<isize> = Vec::new();
        // SAFETY: as above.
        let _ = unsafe {
            EnumChildWindows(
                Some(hwnd(parent)),
                Some(collect),
                LPARAM(&mut found as *mut Vec<isize> as isize),
            )
        };
        found
    }

    pub fn control(target: isize) -> Control {
        let mut rect = RECT::default();
        // SAFETY: rect is a valid out pointer.
        let _ = unsafe { GetWindowRect(hwnd(target), &mut rect) };
        Control {
            hwnd: target,
            class_name: class_of(target),
            text: text_of(target),
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }
    }

    pub fn set_text(target: isize, value: &str) {
        let mut wide: Vec<u16> = value.encode_utf16().collect();
        wide.push(0);
        send(hwnd(target), WM_SETTEXT, 0, wide.as_ptr() as isize);
    }

    pub fn click(target: isize) {
        send(hwnd(target), BM_CLICK, 0, 0);
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Control;
    pub fn text_of(_: isize) -> String {
        String::new()
    }
    pub fn pid_of(_: isize) -> u32 {
        0
    }
    pub fn is_visible(_: isize) -> bool {
        false
    }
    pub fn class_of(_: isize) -> String {
        String::new()
    }
    pub fn top_level() -> Vec<isize> {
        Vec::new()
    }
    pub fn children(_: isize) -> Vec<isize> {
        Vec::new()
    }
    pub fn control(target: isize) -> Control {
        Control {
            hwnd: target,
            class_name: String::new(),
            text: String::new(),
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        }
    }
    pub fn set_text(_: isize, _: &str) {}
    pub fn click(_: isize) {}
}

/// Visible top-level windows belonging to *pid*.
///
/// Filtered by owner before anything is read, so no message is ever sent to
/// another program's window. That is what keeps a hung window elsewhere on
/// the desktop from becoming this program's problem.
pub fn windows_of(pid: u32) -> Vec<isize> {
    imp::top_level()
        .into_iter()
        .filter(|&window| imp::pid_of(window) == pid && imp::is_visible(window))
        .collect()
}

/// The visible top-level window with this title, or None. Given a process
/// id, only that process's windows are looked at.
pub fn find_window(title: &str, pid: Option<u32>) -> Option<isize> {
    let candidates = match pid {
        Some(pid) => windows_of(pid),
        None => imp::top_level()
            .into_iter()
            .filter(|&window| imp::is_visible(window))
            .collect(),
    };
    candidates
        .into_iter()
        .find(|&window| imp::text_of(window) == title)
}

/// A visible message box belonging to *pid*: its window, its text, and its
/// OK/Yes/Close button if it has one. Filtered by process id because #32770
/// is the standard dialog class and every other program uses it too.
pub fn message_box(pid: u32, ignore: &[isize]) -> Option<(isize, String, Option<Control>)> {
    let window = windows_of(pid)
        .into_iter()
        .filter(|window| !ignore.contains(window))
        .find(|&window| imp::class_of(window) == "#32770")?;
    let found = controls(window);
    let message = found
        .iter()
        .filter(|c| c.class_name == "Static" && !c.text.trim().is_empty())
        .map(|c| c.text.trim().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let ok = found
        .iter()
        .find(|c| c.is_button() && matches!(c.text.trim(), "OK" | "Yes" | "Close"))
        .cloned();
    Some((window, message, ok))
}

/// Every child control of *parent*, at any depth.
pub fn controls(parent: isize) -> Vec<Control> {
    imp::children(parent)
        .into_iter()
        .map(imp::control)
        .collect()
}

/// Type a value into a box and read it back. True when it took.
pub fn set_text(control: &Control, value: &str) -> bool {
    imp::set_text(control.hwnd, value);
    imp::text_of(control.hwnd).trim() == value.trim()
}

pub fn read_text(control: &Control) -> String {
    imp::text_of(control.hwnd).trim().to_string()
}

pub fn click(control: &Control) {
    imp::click(control.hwnd);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(class_name: &str, text: &str, left: i32, top: i32) -> Control {
        Control {
            hwnd: 0,
            class_name: class_name.into(),
            text: text.into(),
            left,
            top,
            right: left + 50,
            bottom: top + 20,
        }
    }

    #[test]
    fn box_beside_finds_the_box_on_the_same_line() {
        let found = vec![
            control("WindowsForms10.STATIC.app.0.1", "Max Errors", 10, 100),
            control("WindowsForms10.Edit.app.0.1", "", 120, 40),
            control("WindowsForms10.Edit.app.0.1", "5", 120, 102),
            control("Edit", "", 0, 100),
            control("Static", "Number of cores to &torture test:", 10, 200),
            control("Edit", "8", 200, 198),
        ];
        assert_eq!(
            box_beside(&found, "Max Errors", BOX_TOLERANCE)
                .unwrap()
                .text,
            "5"
        );
        assert_eq!(
            box_beside(&found, "Number of cores to torture test:", BOX_TOLERANCE)
                .unwrap()
                .text,
            "8"
        );
        assert!(box_beside(&found, "Missing", BOX_TOLERANCE).is_none());
    }

    #[test]
    fn control_kinds() {
        let plain = control("Button", "OK", 0, 0);
        assert!(plain.is_button());
        assert_eq!(plain.kind(), "Button");
        let forms = control("WindowsForms10.Button.app.0.2", "Start", 0, 0);
        assert!(forms.is_button());
        assert_eq!(forms.kind(), "Button");
        assert!(button(&[plain.clone(), forms.clone()], "Start").is_some());
        assert!(control("WindowsForms10.Edit.app.0", "", 0, 0).is_edit());
    }

    #[test]
    fn our_process_has_no_visible_windows_in_tests() {
        // Exercises the enumeration path without depending on the desktop.
        let _ = windows_of(std::process::id());
        let _ = find_window("no window is called this, surely", Some(std::process::id()));
        assert!(message_box(std::process::id(), &[]).is_none());
    }
}
