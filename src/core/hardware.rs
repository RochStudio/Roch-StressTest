//! What the machine is, in the few numbers a stress test needs to size itself.
//!
//! Every default in this program is derived from these: how many threads
//! Linpack should use, how big a problem fits. Reading them wrong is worse
//! than not reading them -- a Linpack size chosen from an over-reported
//! free-memory figure swaps rather than stresses -- so each one falls back to
//! a conservative answer rather than a guess.
//!
//! No third-party dependency beyond the `windows` bindings: kernel32 plus one
//! registry read, the same calls the Python version made through ctypes.

#[cfg(windows)]
mod imp {
    use std::mem::size_of;

    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    use windows::Win32::System::SystemInformation::{
        GetLogicalProcessorInformation, GlobalMemoryStatusEx, RelationProcessorCore,
        MEMORYSTATUSEX, SYSTEM_LOGICAL_PROCESSOR_INFORMATION,
    };
    use windows::Win32::System::Threading::GetActiveProcessorCount;

    pub fn memory_status() -> Option<(u64, u64)> {
        let mut status = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: the structure is initialised with its own size, as required.
        unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
        Some((status.ullTotalPhys, status.ullAvailPhys))
    }

    pub fn logical_cores() -> usize {
        // ALL_PROCESSOR_GROUPS, which is what os.cpu_count() asks for.
        // SAFETY: no pointers involved.
        let count = unsafe { GetActiveProcessorCount(0xFFFF) };
        if count > 0 {
            count as usize
        } else {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        }
    }

    pub fn physical_cores() -> Option<usize> {
        let mut length = 0u32;
        // SAFETY: a null buffer asks for the required length only.
        let _ = unsafe { GetLogicalProcessorInformation(None, &mut length) };
        if length == 0 {
            return None;
        }
        let count = length as usize / size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>();
        let mut buffer = vec![SYSTEM_LOGICAL_PROCESSOR_INFORMATION::default(); count.max(1)];
        // SAFETY: the buffer holds `length` bytes, as just reported.
        unsafe { GetLogicalProcessorInformation(Some(buffer.as_mut_ptr()), &mut length) }.ok()?;
        let filled = length as usize / size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>();
        let cores = buffer[..filled.min(buffer.len())]
            .iter()
            .filter(|entry| entry.Relationship == RelationProcessorCore)
            .count();
        (cores > 0).then_some(cores)
    }

    pub fn processor_string(value: windows::core::PCWSTR) -> Option<String> {
        let key = w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");
        let mut size = 0u32;
        // SAFETY: first call sizes the buffer; second fills it.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key,
                value,
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut size),
            )
        };
        if status.is_err() || size == 0 {
            return None;
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2) + 1];
        let mut bytes = (buffer.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key,
                value,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if status.is_err() {
            return None;
        }
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..end]).trim().to_string())
    }

    pub fn vendor() -> Option<String> {
        processor_string(w!("VendorIdentifier"))
    }

    pub fn name() -> Option<String> {
        processor_string(w!("ProcessorNameString"))
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn memory_status() -> Option<(u64, u64)> {
        None
    }
    pub fn logical_cores() -> usize {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    }
    pub fn physical_cores() -> Option<usize> {
        None
    }
    pub fn vendor() -> Option<String> {
        None
    }
    pub fn name() -> Option<String> {
        None
    }
}

const MB: u64 = 1024 * 1024;

/// Installed memory in MB, or 8192 when it cannot be read.
pub fn total_ram_mb() -> i64 {
    match imp::memory_status() {
        Some((total, _)) => (total / MB) as i64,
        None => 8192,
    }
}

/// Free memory in MB right now, or half of installed when unreadable.
pub fn available_ram_mb() -> i64 {
    match imp::memory_status() {
        Some((_, available)) => (available / MB) as i64,
        None => total_ram_mb() / 2,
    }
}

/// Logical processors, which is what a thread count is counted in.
pub fn logical_cores() -> i64 {
    imp::logical_cores().max(1) as i64
}

/// Physical cores, counted from the processor-relationship table.
///
/// Falls back to half the logical count -- the answer for every SMT machine
/// and wrong only in the direction of asking for fewer workers -- when the
/// table cannot be read. GetLogicalProcessorInformation describes only the
/// first processor group, so on a machine with more than 64 logical
/// processors this undercounts, exactly as the Python version did.
pub fn physical_cores() -> i64 {
    match imp::physical_cores() {
        Some(cores) => cores as i64,
        None => (logical_cores() / 2).max(1),
    }
}

/// "AuthenticAMD", "GenuineIntel", or "" when the key is unreadable.
pub fn cpu_vendor() -> String {
    imp::vendor().unwrap_or_default()
}

/// The marketing string, for the run header in the log.
pub fn cpu_name() -> String {
    imp::name().unwrap_or_else(|| "Unknown CPU".to_string())
}

pub fn is_amd() -> bool {
    cpu_vendor().to_uppercase() == "AUTHENTICAMD"
}

/// One line naming the machine, written at the top of every run log.
pub fn describe() -> String {
    format!(
        "{} -- {}C/{}T, {:.1} GB RAM",
        cpu_name(),
        physical_cores(),
        logical_cores(),
        total_ram_mb() as f64 / 1024.0
    )
}

/// The same line without the memory size, for the toolbar, which shows memory
/// live a few pixels to the right.
pub fn describe_cpu() -> String {
    format!(
        "{} -- {}C/{}T",
        cpu_name(),
        physical_cores(),
        logical_cores()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_are_sane() {
        assert!(total_ram_mb() > 0);
        let available = available_ram_mb();
        assert!(available > 0 && available <= total_ram_mb());
        assert!(logical_cores() >= 1);
        assert!(physical_cores() >= 1 && physical_cores() <= logical_cores());
        assert!(!cpu_name().is_empty());
        assert!(describe().contains("GB RAM"));
        assert!(describe_cpu().contains("C/"));
    }
}
