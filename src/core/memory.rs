//! Freeing memory back to the system, and watching how much there is.
//!
//! A memory test is only testing memory it can actually get. Windows keeps a
//! standby list -- pages it has finished with but is holding in case they are
//! wanted again -- and counts them as "in use" for the purposes of an
//! allocation. Ask y-cruncher for 28 GB on a machine that has been up for a
//! week and a good part of that request goes to the page file, so the run
//! measures an SSD rather than the DIMMs.
//!
//! Three operations free it, all through the same undocumented-but-stable call
//! every memory cleaner uses, `NtSetSystemInformation` with
//! SystemMemoryListInformation:
//!
//! ```text
//!     empty working sets            trims every process to its minimum
//!     purge the standby list        the big one, and the reason for this file
//!     purge the low-priority list   the cheap half of the same thing
//! ```
//!
//! They need SeProfileSingleProcessPrivilege, which an administrator has but
//! which is not enabled in the token by default; it has to be turned on first.
//! This is the same sequence as danskee's Memory Cleaner, whose DLL exports one
//! CleanMemory and imports exactly NtSetSystemInformation,
//! AdjustTokenPrivileges and LookupPrivilegeValue.
//!
//! Nothing here is destructive. Purging the standby list discards cached
//! copies of data that is already on disk; the pages are re-read if they are
//! wanted again.

use crate::core::hardware;
use crate::core::pyutil::thousands;

/// SYSTEM_INFORMATION_CLASS
pub const SYSTEM_MEMORY_LIST_INFORMATION: i32 = 0x0050;

/// SYSTEM_MEMORY_LIST_COMMAND
pub const MEMORY_EMPTY_WORKING_SETS: i32 = 2;
pub const MEMORY_FLUSH_MODIFIED_LIST: i32 = 3;
pub const MEMORY_PURGE_STANDBY_LIST: i32 = 4;
pub const MEMORY_PURGE_LOW_PRIORITY_STANDBY_LIST: i32 = 5;

/// What each step is called in the report, in the order they are run. Working
/// sets first: trimming processes moves their pages onto the standby list,
/// where the next step can then free them. The other order frees less.
pub const STEPS: &[(&str, i32)] = &[
    ("working sets", MEMORY_EMPTY_WORKING_SETS),
    ("standby list", MEMORY_PURGE_STANDBY_LIST),
    (
        "low-priority standby list",
        MEMORY_PURGE_LOW_PRIORITY_STANDBY_LIST,
    ),
];

#[cfg(windows)]
mod imp {
    use std::mem::size_of;

    use windows::core::{s, w, HSTRING};
    use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, LUID, WIN32_ERROR};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// Turn on one privilege in this process's token. True when it took.
    ///
    /// Being an administrator is not the same as having the privilege
    /// enabled: it is present but disabled until asked for, and
    /// NtSetSystemInformation fails with a plain access-denied if it is not.
    pub fn enable_privilege(name: &str) -> bool {
        let mut token = HANDLE::default();
        // SAFETY: plain Win32 calls on our own process token; every handle
        // opened here is closed before returning.
        unsafe {
            if OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
            .is_err()
            {
                return false;
            }
            let result = (|| {
                let mut luid = LUID::default();
                let wide = HSTRING::from(name);
                if LookupPrivilegeValueW(None, &wide, &mut luid).is_err() {
                    return false;
                }
                let privileges = TOKEN_PRIVILEGES {
                    PrivilegeCount: 1,
                    Privileges: [LUID_AND_ATTRIBUTES {
                        Luid: luid,
                        Attributes: SE_PRIVILEGE_ENABLED,
                    }],
                };
                if AdjustTokenPrivileges(
                    token,
                    false,
                    Some(&privileges),
                    size_of::<TOKEN_PRIVILEGES>() as u32,
                    None,
                    None,
                )
                .is_err()
                {
                    return false;
                }
                // AdjustTokenPrivileges reports success even when it changed
                // nothing, so the real answer is in the last error.
                GetLastError() == WIN32_ERROR(0)
            })();
            let _ = CloseHandle(token);
            result
        }
    }

    type NtSetSystemInformation =
        unsafe extern "system" fn(i32, *mut core::ffi::c_void, u32) -> i32;

    /// Issue one SystemMemoryListInformation command. True on success.
    pub fn set_memory_list(command: i32) -> bool {
        // SAFETY: ntdll is always loaded; the function pointer is the
        // documented export with the documented signature, and the buffer is
        // a single i32 as SystemMemoryListInformation expects.
        unsafe {
            let Ok(ntdll) = GetModuleHandleW(w!("ntdll.dll")) else {
                return false;
            };
            let Some(address) = GetProcAddress(ntdll, s!("NtSetSystemInformation")) else {
                return false;
            };
            let call: NtSetSystemInformation = std::mem::transmute(address);
            let mut value: i32 = command;
            let status = call(
                super::SYSTEM_MEMORY_LIST_INFORMATION,
                (&mut value as *mut i32).cast(),
                size_of::<i32>() as u32,
            );
            status == 0
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn enable_privilege(_name: &str) -> bool {
        false
    }
    pub fn set_memory_list(_command: i32) -> bool {
        false
    }
}

pub use imp::enable_privilege;

/// What a clean actually achieved.
#[derive(Debug, Clone, PartialEq)]
pub struct CleanResult {
    pub before_mb: i64,
    pub after_mb: i64,
    pub done: Vec<String>,
    pub failed: Vec<String>,
}

impl CleanResult {
    pub fn freed_mb(&self) -> i64 {
        (self.after_mb - self.before_mb).max(0)
    }

    pub fn describe(&self) -> String {
        let mut text = format!(
            "Memory cleaned: {} MB free before, {} MB after",
            thousands(self.before_mb),
            thousands(self.after_mb)
        );
        if self.freed_mb() > 0 {
            text += &format!(" ({} MB freed)", thousands(self.freed_mb()));
        } else {
            text += " (nothing to free)";
        }
        if !self.failed.is_empty() {
            text += &format!(". Could not clear: {}", self.failed.join(", "));
        }
        text
    }
}

/// Empty working sets and purge the standby lists.
///
/// Returns what was and was not done rather than an error: failing to free
/// memory is a disappointment, not a reason to stop a stress test starting.
pub fn clean() -> CleanResult {
    let before = hardware::available_ram_mb();
    enable_privilege("SeProfileSingleProcessPrivilege");
    enable_privilege("SeIncreaseQuotaPrivilege");

    let mut done = Vec::new();
    let mut failed = Vec::new();
    for (label, command) in STEPS {
        if imp::set_memory_list(*command) {
            done.push(label.to_string());
        } else {
            failed.push(label.to_string());
        }
    }
    CleanResult {
        before_mb: before,
        after_mb: hardware::available_ram_mb(),
        done,
        failed,
    }
}

/// (available MB, total MB, percent used) for the live display.
pub fn reading() -> (i64, i64, i64) {
    let total = hardware::total_ram_mb();
    let available = hardware::available_ram_mb();
    (available, total, percent_used(available, total))
}

/// Python's `int(round(x))` rounds halves to even; so does this.
fn percent_used(available: i64, total: i64) -> i64 {
    if total == 0 {
        return 0;
    }
    let value = (total - available) as f64 * 100.0 / total as f64;
    value.round_ties_even() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_reads_like_the_original() {
        let result = CleanResult {
            before_mb: 10_000,
            after_mb: 12_500,
            done: vec!["working sets".into()],
            failed: vec![],
        };
        assert_eq!(
            result.describe(),
            "Memory cleaned: 10,000 MB free before, 12,500 MB after (2,500 MB freed)"
        );
        let result = CleanResult {
            before_mb: 900,
            after_mb: 800,
            done: vec![],
            failed: vec!["standby list".into(), "low-priority standby list".into()],
        };
        assert_eq!(
            result.describe(),
            "Memory cleaned: 900 MB free before, 800 MB after (nothing to free). \
             Could not clear: standby list, low-priority standby list"
        );
    }

    #[test]
    fn percentages() {
        assert_eq!(percent_used(50, 100), 50);
        assert_eq!(percent_used(0, 0), 0);
        assert_eq!(percent_used(1, 8), 88);
        let (available, total, used) = reading();
        assert!(available <= total);
        assert!((0..=100).contains(&used));
    }

    #[test]
    fn steps_run_working_sets_first() {
        assert_eq!(STEPS[0].1, MEMORY_EMPTY_WORKING_SETS);
        assert_eq!(STEPS.len(), 3);
        let _ = MEMORY_FLUSH_MODIFIED_LIST;
    }
}
