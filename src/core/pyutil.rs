//! Small helpers that reproduce the Python standard-library behaviour the
//! original program leaned on, so that log parsing, formatting and file names
//! come out the same as they did.

/// `str.splitlines()`: split on every line boundary Python recognises, and
/// drop the terminators. A trailing terminator does not produce an empty
/// final line, exactly as in Python.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        let boundary = matches!(
            ch,
            '\n' | '\r'
                | '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !boundary {
            continue;
        }
        lines.push(&text[start..index]);
        let mut end = index + ch.len_utf8();
        if ch == '\r' {
            if let Some(&(next_index, '\n')) = chars.peek() {
                chars.next();
                end = next_index + 1;
            }
        }
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Python's whitespace test for `str.strip()`, which is wider than Rust's:
/// it also counts the four ASCII separator controls.
fn is_py_space(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// `str.strip()`.
pub fn strip(text: &str) -> &str {
    text.trim_matches(is_py_space)
}

/// `str.rstrip()`.
pub fn rstrip(text: &str) -> &str {
    text.trim_end_matches(is_py_space)
}

/// `str.split()` with no argument: runs of whitespace separate, and there
/// are no empty pieces.
pub fn split_ws(text: &str) -> Vec<&str> {
    text.split(is_py_space)
        .filter(|piece| !piece.is_empty())
        .collect()
}

/// `"{:,}".format(n)`: thousands separated by commas.
pub fn thousands(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if value < 0 {
        out.push('-');
    }
    let first = digits.len() % 3;
    for (index, ch) in digits.chars().enumerate() {
        if index != 0 && (index + 3 - first).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Text read the way Python's `open(path)` read it on Windows: in the ANSI
/// code page, with undecodable bytes replaced. The .bat files y-cruncher
/// profiles are kept in are read and written this way.
pub fn decode_ansi(bytes: &[u8]) -> String {
    #[cfg(windows)]
    {
        use windows::Win32::Globalization::{
            MultiByteToWideChar, CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS,
        };
        if bytes.is_empty() {
            return String::new();
        }
        // SAFETY: both calls are given correctly sized slices.
        unsafe {
            let needed = MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None);
            if needed > 0 {
                let mut wide = vec![0u16; needed as usize];
                let written = MultiByteToWideChar(
                    CP_ACP,
                    MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0),
                    bytes,
                    Some(&mut wide),
                );
                if written > 0 {
                    return String::from_utf16_lossy(&wide[..written as usize]);
                }
            }
        }
        String::from_utf8_lossy(bytes).into_owned()
    }
    #[cfg(not(windows))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// The reverse of `decode_ansi`, for writing a file the way Python did.
pub fn encode_ansi(text: &str) -> Vec<u8> {
    #[cfg(windows)]
    {
        use windows::core::PCSTR;
        use windows::Win32::Globalization::{WideCharToMultiByte, CP_ACP};
        if text.is_empty() {
            return Vec::new();
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: both calls are given correctly sized slices.
        unsafe {
            let needed = WideCharToMultiByte(CP_ACP, 0, &wide, None, PCSTR::null(), None);
            if needed > 0 {
                let mut out = vec![0u8; needed as usize];
                let written =
                    WideCharToMultiByte(CP_ACP, 0, &wide, Some(&mut out), PCSTR::null(), None);
                if written > 0 {
                    out.truncate(written as usize);
                    return out;
                }
            }
        }
        text.as_bytes().to_vec()
    }
    #[cfg(not(windows))]
    {
        text.as_bytes().to_vec()
    }
}

/// The local time as `time.strftime` would give it for the few formats the
/// program uses: `%Y%m%d-%H%M%S`.
pub fn local_timestamp() -> String {
    #[cfg(windows)]
    {
        // SAFETY: GetLocalTime has no preconditions.
        let now = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond
        )
    }
    #[cfg(not(windows))]
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("{secs}")
    }
}

/// Seconds as h:mm:ss, which is how a stress run is actually read.
pub fn format_duration(seconds: f64) -> String {
    let seconds = if seconds.is_finite() && seconds > 0.0 {
        seconds as u64
    } else {
        0
    };
    let hours = seconds / 3600;
    let rest = seconds % 3600;
    format!("{}:{:02}:{:02}", hours, rest / 60, rest % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_matches_python() {
        assert_eq!(splitlines("a\nb\r\nc\rd"), vec!["a", "b", "c", "d"]);
        assert_eq!(splitlines("a\n"), vec!["a"]);
        assert_eq!(splitlines("a\n\nb"), vec!["a", "", "b"]);
        assert_eq!(splitlines(""), Vec::<&str>::new());
        assert_eq!(splitlines("\r\n"), vec![""]);
        assert_eq!(splitlines("x\u{2028}y"), vec!["x", "y"]);
    }

    #[test]
    fn thousands_matches_python() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(32_768), "32,768");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(-1234), "-1,234");
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration(0.0), "0:00:00");
        assert_eq!(format_duration(59.9), "0:00:59");
        assert_eq!(format_duration(3661.0), "1:01:01");
        assert_eq!(format_duration(-5.0), "0:00:00");
        assert_eq!(format_duration(36000.0 + 120.0), "10:02:00");
    }

    #[test]
    fn whitespace_split() {
        assert_eq!(split_ws("  a  b\tc \n"), vec!["a", "b", "c"]);
        assert_eq!(strip("  x \r\n"), "x");
        assert_eq!(rstrip("  x \r\n"), "  x");
    }

    #[test]
    fn ansi_round_trip() {
        let text = "y-cruncher.exe pause:1 stress -TL:1800 VT3";
        assert_eq!(decode_ansi(&encode_ansi(text)), text);
        assert_eq!(decode_ansi(b""), "");
    }

    #[test]
    fn timestamp_shape() {
        let stamp = local_timestamp();
        assert_eq!(stamp.len(), 15);
        assert_eq!(&stamp[8..9], "-");
    }
}
