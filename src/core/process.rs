//! Starting a tool's process exactly the way Python's `subprocess.Popen` did.
//!
//! Rust's `std::process::Command` always hands the child a set of standard
//! handles (STARTF_USESTDHANDLES), inherited from this process when nothing
//! else is asked for. For a console tool given a console of its own
//! (CREATE_NEW_CONSOLE) that is exactly wrong: its output goes to handles that
//! belong to us -- nothing at all, in a windowed program -- instead of to the
//! console it was just given. y-cruncher, started that way, crashed and left a
//! minidump beside itself.
//!
//! Popen only passes standard handles when one of them is redirected. So this
//! calls CreateProcessW directly and does the same: no handles at all for a
//! windowed or own-console tool, and a pipe (stdout and stderr together, stdin
//! from NUL) for a tool whose output is read straight off the process. The
//! command line is built with Popen's own quoting (`list2cmdline`), or taken
//! verbatim when a tool supplies a raw one.

use std::io;
use std::path::Path;

/// `subprocess.list2cmdline`: the quoting the Microsoft C runtime expects,
/// exactly as Python applied it.
pub fn list2cmdline<S: AsRef<str>>(args: &[S]) -> String {
    let mut result = String::new();
    for arg in args {
        let arg = arg.as_ref();
        let mut backslashes = String::new();
        if !result.is_empty() {
            result.push(' ');
        }
        let quote = arg.contains(' ') || arg.contains('\t') || arg.is_empty();
        if quote {
            result.push('"');
        }
        for ch in arg.chars() {
            match ch {
                '\\' => backslashes.push(ch),
                '"' => {
                    result.push_str(&"\\".repeat(backslashes.len() * 2));
                    backslashes.clear();
                    result.push_str("\\\"");
                }
                _ => {
                    if !backslashes.is_empty() {
                        result.push_str(&backslashes);
                        backslashes.clear();
                    }
                    result.push(ch);
                }
            }
        }
        if !backslashes.is_empty() {
            result.push_str(&backslashes);
        }
        if quote {
            result.push_str(&backslashes);
            result.push('"');
        }
    }
    result
}

/// What to start, and how.
pub struct SpawnOptions<'a> {
    /// The whole command line, as CreateProcess takes it.
    pub command_line: String,
    pub cwd: &'a Path,
    /// Variables set on top of this program's own environment.
    pub env: &'a [(String, String)],
    pub creation_flags: u32,
    /// Read stdout and stderr through a pipe, with stdin from NUL.
    pub pipe: bool,
}

#[cfg(windows)]
mod imp {
    use super::SpawnOptions;
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
    };
    use windows::Win32::System::Threading::{
        CreateProcessW, GetExitCodeProcess, TerminateProcess, WaitForSingleObject,
        CREATE_UNICODE_ENVIRONMENT, INFINITE, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION,
        STARTF_USESTDHANDLES, STARTUPINFOW,
    };

    /// A started process: its handle and id. Closed when dropped.
    pub struct Process {
        handle: isize,
        pid: u32,
        exit: Option<u32>,
    }

    // SAFETY: a process handle may be used from any thread.
    unsafe impl Send for Process {}

    impl Process {
        fn raw(&self) -> HANDLE {
            HANDLE(self.handle as *mut core::ffi::c_void)
        }

        pub fn id(&self) -> u32 {
            self.pid
        }

        /// The exit code if it has exited, without waiting.
        pub fn try_wait(&mut self) -> io::Result<Option<u32>> {
            if let Some(code) = self.exit {
                return Ok(Some(code));
            }
            // SAFETY: our own process handle, open until drop.
            unsafe {
                if WaitForSingleObject(self.raw(), 0) != WAIT_OBJECT_0 {
                    return Ok(None);
                }
                let mut code = 0u32;
                GetExitCodeProcess(self.raw(), &mut code).map_err(io::Error::other)?;
                self.exit = Some(code);
                Ok(Some(code))
            }
        }

        pub fn wait(&mut self) -> io::Result<u32> {
            // SAFETY: as above.
            unsafe {
                WaitForSingleObject(self.raw(), INFINITE);
            }
            self.try_wait()?
                .ok_or_else(|| io::Error::other("process did not exit"))
        }

        pub fn kill(&mut self) -> io::Result<()> {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            // SAFETY: as above.
            unsafe { TerminateProcess(self.raw(), 1) }.map_err(io::Error::other)
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: closing the handle we own.
            unsafe {
                let _ = CloseHandle(self.raw());
            }
        }
    }

    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// This program's environment with *extra* laid over it, as a
    /// CreateProcess block: KEY=value, each NUL-terminated, sorted, with a
    /// final NUL. Names compare case-insensitively, as Windows does.
    fn environment_block(extra: &[(String, String)]) -> Vec<u16> {
        let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        for (key, value) in extra {
            vars.retain(|(k, _)| !k.to_string_lossy().eq_ignore_ascii_case(key));
            vars.push((key.into(), value.into()));
        }
        vars.sort_by_key(|(k, _)| k.to_string_lossy().to_uppercase());
        let mut block = Vec::new();
        for (key, value) in vars {
            block.extend(key.encode_wide());
            block.push(u16::from(b'='));
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.push(0);
        block
    }

    fn inheritable(handle: HANDLE) -> io::Result<()> {
        // SAFETY: a handle we own.
        unsafe {
            SetHandleInformation(
                handle,
                HANDLE_FLAG_INHERIT.0,
                HANDLE_FLAGS(HANDLE_FLAG_INHERIT.0),
            )
        }
        .map_err(io::Error::other)
    }

    pub fn spawn(options: &SpawnOptions<'_>) -> io::Result<(Process, Option<std::io::PipeReader>)> {
        let mut command_line: Vec<u16> = options
            .command_line
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let cwd = wide(options.cwd.as_os_str());
        let environment = if options.env.is_empty() {
            None
        } else {
            Some(environment_block(options.env))
        };

        let mut startup = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };

        // Held until CreateProcess has duplicated them into the child.
        let mut reader = None;
        let mut child_ends: Vec<Box<dyn AsRawHandle>> = Vec::new();
        if options.pipe {
            let (pipe_reader, pipe_writer) = std::io::pipe()?;
            let null = std::fs::OpenOptions::new().read(true).open("NUL")?;
            let output = HANDLE(pipe_writer.as_raw_handle());
            let input = HANDLE(null.as_raw_handle());
            inheritable(output)?;
            inheritable(input)?;
            startup.dwFlags |= STARTF_USESTDHANDLES;
            startup.hStdInput = input;
            startup.hStdOutput = output;
            startup.hStdError = output;
            child_ends.push(Box::new(pipe_writer));
            child_ends.push(Box::new(null));
            reader = Some(pipe_reader);
        }

        let mut flags = PROCESS_CREATION_FLAGS(options.creation_flags);
        if environment.is_some() {
            flags |= CREATE_UNICODE_ENVIRONMENT;
        }
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: every buffer outlives the call; the command line buffer is
        // writable, as CreateProcessW requires.
        let created = unsafe {
            CreateProcessW(
                PCWSTR::null(),
                Some(PWSTR(command_line.as_mut_ptr())),
                None,
                None,
                options.pipe,
                flags,
                environment
                    .as_ref()
                    .map(|block| block.as_ptr() as *const core::ffi::c_void),
                PCWSTR(cwd.as_ptr()),
                &startup,
                &mut info,
            )
        };
        // Our copies of the child's ends go now, or the reader never sees the
        // end of the output.
        drop(child_ends);
        created.map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF))?;
        // SAFETY: the thread handle is ours and not needed.
        unsafe {
            let _ = CloseHandle(info.hThread);
        }
        Ok((
            Process {
                handle: info.hProcess.0 as isize,
                pid: info.dwProcessId,
                exit: None,
            },
            reader,
        ))
    }
}

#[cfg(windows)]
pub use imp::{spawn, Process};

/// Not Windows: the std launcher, for the sake of compiling and testing.
#[cfg(not(windows))]
mod imp {
    use super::SpawnOptions;
    use std::io;
    use std::process::{Child, Command, Stdio};

    pub struct Process(Child);

    impl Process {
        pub fn id(&self) -> u32 {
            self.0.id()
        }
        pub fn try_wait(&mut self) -> io::Result<Option<u32>> {
            Ok(self.0.try_wait()?.map(|s| s.code().unwrap_or(1) as u32))
        }
        pub fn wait(&mut self) -> io::Result<u32> {
            Ok(self.0.wait()?.code().unwrap_or(1) as u32)
        }
        pub fn kill(&mut self) -> io::Result<()> {
            self.0.kill()
        }
    }

    pub fn spawn(options: &SpawnOptions<'_>) -> io::Result<(Process, Option<std::io::PipeReader>)> {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(&options.command_line)
            .current_dir(options.cwd);
        for (key, value) in options.env {
            command.env(key, value);
        }
        let mut reader = None;
        if options.pipe {
            let (r, w) = std::io::pipe()?;
            command
                .stdin(Stdio::null())
                .stdout(w.try_clone()?)
                .stderr(w);
            reader = Some(r);
        }
        let child = command.spawn()?;
        drop(command);
        Ok((Process(child), reader))
    }
}

#[cfg(not(windows))]
pub use imp::{spawn, Process};

/// Start *options*; the error is what Python's OSError would have said.
pub fn start(options: &SpawnOptions<'_>) -> io::Result<(Process, Option<std::io::PipeReader>)> {
    spawn(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_matches_list2cmdline() {
        assert_eq!(list2cmdline(&["a", "b c", ""]), "a \"b c\" \"\"");
        assert_eq!(
            list2cmdline(&[r"C:\Program Files\x\y.exe", "-Wc:\\dir"]),
            "\"C:\\Program Files\\x\\y.exe\" -Wc:\\dir"
        );
        assert_eq!(list2cmdline(&["say \"hi\""]), "\"say \\\"hi\\\"\"");
        assert_eq!(list2cmdline(&[r"ends\"]), r"ends\");
        assert_eq!(
            list2cmdline(&[r"ends with space\ "]),
            "\"ends with space\\ \""
        );
        assert_eq!(list2cmdline(&[r"dir\ x\"]), "\"dir\\ x\\\\\"");
        assert_eq!(list2cmdline(&[r#"a\"b"#]), r#"a\\\"b"#);
        assert_eq!(
            list2cmdline(&["logfile:C:\\Users\\me\\AppData\\Local\\RochStressTest\\runs\\ycruncher\\ycruncher.log"]),
            "logfile:C:\\Users\\me\\AppData\\Local\\RochStressTest\\runs\\ycruncher\\ycruncher.log"
        );
    }

    #[test]
    fn a_piped_child_is_read_and_its_exit_code_kept() {
        use std::io::Read;
        let cwd = std::env::temp_dir();
        let env = vec![("ROCH_TEST_VALUE".to_string(), "seen".to_string())];
        let (mut process, reader) = start(&SpawnOptions {
            command_line: "cmd /d /c \"echo %ROCH_TEST_VALUE% & echo oops 1>&2 & exit 7\"".into(),
            cwd: &cwd,
            env: &env,
            creation_flags: crate::core::toolbase::CREATE_NO_WINDOW,
            pipe: true,
        })
        .unwrap();
        let mut text = String::new();
        reader.unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(process.wait().unwrap(), 7);
        assert!(text.contains("seen"), "{text}");
        assert!(text.contains("oops"), "{text}");
        assert!(process.id() > 0);
    }

    #[test]
    fn an_unpiped_child_runs_and_can_be_killed() {
        let cwd = std::env::temp_dir();
        let (mut process, reader) = start(&SpawnOptions {
            command_line: "ping -n 30 127.0.0.1".into(),
            cwd: &cwd,
            env: &[],
            creation_flags: crate::core::toolbase::CREATE_NO_WINDOW,
            pipe: false,
        })
        .unwrap();
        assert!(reader.is_none());
        assert_eq!(process.try_wait().unwrap(), None);
        process.kill().unwrap();
        assert_eq!(process.wait().unwrap(), 1);
    }

    #[test]
    fn a_missing_program_is_an_error() {
        let cwd = std::env::temp_dir();
        let result = start(&SpawnOptions {
            command_line: "\"C:\\no\\such\\program.exe\"".into(),
            cwd: &cwd,
            env: &[],
            creation_flags: 0,
            pipe: false,
        });
        assert!(result.is_err());
    }
}
