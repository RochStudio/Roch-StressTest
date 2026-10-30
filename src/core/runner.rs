//! Starting a test, watching it, and deciding when it is over.
//!
//! This is the part that makes the program worth more than ten shortcuts.
//! Every tool is supervised the same way:
//!
//!   * its output is collected -- from the pipe when it has one, from its log
//!     files when it does not,
//!   * every line is checked against that tool's failure patterns,
//!   * a wall-clock limit is enforced, because most of these tools cannot
//!     limit themselves,
//!   * and the process tree is killed properly when the run ends, whichever
//!     way it ends.
//!
//! Everything here runs on a worker thread and reports back through a
//! channel. Nothing touches the window: the UI drains the channel on its own
//! thread, and that boundary is the only threading rule in the program.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::core::errors;
use crate::core::process::{self, list2cmdline, Process, SpawnOptions};
use crate::core::pyutil::{rstrip, splitlines};
use crate::core::toolbase::{LaunchSpec, CREATE_NO_WINDOW};
use crate::core::winui;

/// How a run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunState {
    Idle,
    Running,
    Passed,
    Failed,
    Stopped,
    Broken,
}

impl RunState {
    /// The name the Python version used for the state.
    pub fn key(self) -> &'static str {
        match self {
            RunState::Idle => "idle",
            RunState::Running => "running",
            RunState::Passed => "passed",
            RunState::Failed => "failed",
            RunState::Stopped => "stopped",
            RunState::Broken => "broken",
        }
    }

    /// What the status strip says.
    pub fn text(self) -> &'static str {
        match self {
            RunState::Idle => "Idle",
            RunState::Running => "Running",
            RunState::Passed => "Passed",
            RunState::Failed => "FAILED",
            RunState::Stopped => "Stopped",
            RunState::Broken => "Could not start",
        }
    }
}

/// 0xC0000005, an access violation. A windowed tool torn down mid-run often
/// ends this way -- Prime95 closing with eight workers and 28 GB in flight
/// does it every time -- and on its own the code says nothing about which
/// happened. What separates them is whether the window went first.
pub const STATUS_ACCESS_VIOLATION: u32 = 0xC000_0005;

/// 0xC000013A. Windows gives a console process this exit code when it is sent
/// Ctrl+C or Ctrl+Break, or when its console window is closed. It says
/// nothing about the health of the machine, so it must never be reported as a
/// failure.
pub const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;

/// How often the watcher wakes to poll log files and check the clock, for a
/// tool without a pipe. Far below any useful test duration.
pub const POLL_SECONDS: f64 = 2.0;

/// One thing that happened, on its way to the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A line of the tool's output.
    Output(String),
    /// The run changed state.
    State { state: RunState, note: String },
    /// Once a second, for the status strip.
    Tick {
        elapsed: f64,
        remaining: Option<f64>,
    },
    /// A tool-specific running figure.
    Stat(String),
}

/// Reads whatever gets appended to a file after we started watching.
///
/// Files that do not exist yet are fine -- Prime95 creates results.txt only
/// when it has something to say, which is the case we most want to catch.
/// Files that shrink are treated as new files and re-read from the start,
/// since that means the tool rotated or rewrote its log.
pub struct FileTail {
    pub path: PathBuf,
    offset: u64,
    /// Decided from the byte-order mark on the first read. TM5 writes UTF-8
    /// with a BOM, and PowerShell's Tee-Object writes UTF-16LE: read as
    /// UTF-8, a UTF-16 log has a NUL between every character and not one
    /// pattern matches it.
    encoding: Option<Encoding>,
    /// A byte held back because a two-byte character straddled the end of a
    /// read. Without this, one unlucky poll corrupts a line.
    pending: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

impl FileTail {
    pub fn new(path: PathBuf) -> Self {
        let offset = fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .unwrap_or(0);
        FileTail {
            path,
            offset,
            encoding: None,
            pending: Vec::new(),
        }
    }

    /// No mark, but every second byte is a NUL: UTF-16 written without a BOM.
    /// The test is deliberately strict -- a NUL at every odd position across
    /// the first 32 bytes -- because guessing this one wrong would garble an
    /// ordinary log, and a garbled log is a run whose failures match nothing.
    pub fn sniff(chunk: &[u8]) -> Encoding {
        if chunk.starts_with(&[0xFF, 0xFE]) {
            return Encoding::Utf16Le;
        }
        if chunk.starts_with(&[0xFE, 0xFF]) {
            return Encoding::Utf16Be;
        }
        let head = &chunk[..chunk.len().min(32)];
        if head.len() >= 16 && head.iter().skip(1).step_by(2).all(|&b| b == 0) {
            return Encoding::Utf16Le;
        }
        if head.len() >= 16 && head.iter().step_by(2).all(|&b| b == 0) {
            return Encoding::Utf16Be;
        }
        Encoding::Utf8
    }

    fn decode(encoding: Encoding, data: &[u8]) -> String {
        match encoding {
            Encoding::Utf8 => String::from_utf8_lossy(data).into_owned(),
            Encoding::Utf16Le | Encoding::Utf16Be => {
                let units = data.as_chunks::<2>().0.iter().map(|pair| {
                    if encoding == Encoding::Utf16Le {
                        u16::from_le_bytes([pair[0], pair[1]])
                    } else {
                        u16::from_be_bytes([pair[0], pair[1]])
                    }
                });
                char::decode_utf16(units)
                    .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect()
            }
        }
    }

    pub fn read_new(&mut self) -> String {
        let Ok(meta) = fs::metadata(&self.path) else {
            return String::new();
        };
        if !meta.is_file() {
            return String::new();
        }
        let size = meta.len();
        if size < self.offset {
            // Rotated or rewritten: start again, and re-sniff, because it may
            // not be the same kind of file any more.
            self.offset = 0;
            self.encoding = None;
            self.pending.clear();
        }
        if size == self.offset {
            return String::new();
        }
        let mut chunk = Vec::new();
        let read = fs::File::open(&self.path).and_then(|mut handle| {
            handle.seek(SeekFrom::Start(self.offset))?;
            handle.read_to_end(&mut chunk)
        });
        if read.is_err() {
            return String::new();
        }
        self.offset += chunk.len() as u64;

        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(&chunk);
        let encoding = *self.encoding.get_or_insert_with(|| Self::sniff(&data));
        if encoding != Encoding::Utf8 && data.len() % 2 == 1 {
            self.pending = vec![data.pop().unwrap_or_default()];
        }
        Self::decode(encoding, &data)
            .trim_start_matches('\u{feff}')
            .to_string()
    }
}

/// An exit code as both numbers, since one of them is always the clue:
/// 3221225477 is nothing, 0xC0000005 is an access violation.
pub fn exit_code_text(code: u32) -> String {
    format!("code {code} (0x{code:08X})")
}

fn exited(child: &Mutex<Process>) -> Option<u32> {
    let mut guard = child.lock().ok()?;
    guard.try_wait().ok().flatten()
}

/// Stop a process and anything it started.
///
/// Killing the process alone is not enough for the windowed tools, and
/// Prime95 leaves worker threads mid-FFT if it is killed rudely without the
/// tree being taken with it. `taskkill /T /F` is the one thing that reliably
/// ends all of them.
pub fn kill_tree(child: &Mutex<Process>) {
    let pid = {
        let Ok(mut guard) = child.lock() else { return };
        if let Ok(Some(_)) = guard.try_wait() {
            return;
        }
        guard.id()
    };
    let mut command = Command::new("taskkill");
    command
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    if let Ok(mut taskkill) = command.spawn() {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match taskkill.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
                _ => {
                    let _ = taskkill.kill();
                    let _ = taskkill.wait();
                    break;
                }
            }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if exited(child).is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if let Ok(mut guard) = child.lock() {
        let _ = guard.kill();
        let _ = guard.wait();
    }
}

struct Shared {
    sender: Sender<Event>,
    state: Mutex<RunState>,
    finding: Mutex<String>,
}

impl Shared {
    fn emit(&self, event: Event) {
        let _ = self.sender.send(event);
    }

    fn set_state(&self, state: RunState, note: &str) {
        if let Ok(mut guard) = self.state.lock() {
            *guard = state;
        }
        self.emit(Event::State {
            state,
            note: note.to_string(),
        });
    }
}

/// Runs one LaunchSpec at a time and reports through `events`.
pub struct Runner {
    pub events: Receiver<Event>,
    shared: Arc<Shared>,
    child: Option<Arc<Mutex<Process>>>,
    thread: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    started_at: Instant,
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner {
    pub fn new() -> Self {
        let (sender, events) = channel();
        Runner {
            events,
            shared: Arc::new(Shared {
                sender,
                state: Mutex::new(RunState::Idle),
                finding: Mutex::new(String::new()),
            }),
            child: None,
            thread: None,
            stop: Arc::new(AtomicBool::new(false)),
            started_at: Instant::now(),
        }
    }

    pub fn running(&self) -> bool {
        self.thread
            .as_ref()
            .map(|t| !t.is_finished())
            .unwrap_or(false)
    }

    pub fn state(&self) -> RunState {
        self.shared
            .state
            .lock()
            .map(|s| *s)
            .unwrap_or(RunState::Idle)
    }

    /// The failing line of the last run, or "" when it did not fail.
    pub fn finding(&self) -> String {
        self.shared
            .finding
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// The process id of the current or last run.
    pub fn pid(&self) -> Option<u32> {
        self.child
            .as_ref()
            .and_then(|c| c.lock().ok().map(|g| g.id()))
    }

    /// Drain every event waiting, without blocking.
    pub fn drain(&self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            out.push(event);
        }
        out
    }

    /// Launch *spec*. An error if a run is going or the process will not start.
    pub fn start(&mut self, mut spec: LaunchSpec, label: &str) -> Result<(), String> {
        if self.running() {
            return Err("a test is already running".to_string());
        }
        if let Some(old) = self.thread.take() {
            let _ = old.join();
        }
        self.stop.store(false, Ordering::SeqCst);
        if let Ok(mut finding) = self.shared.finding.lock() {
            finding.clear();
        }
        self.started_at = Instant::now();

        // Handed to Windows verbatim when the tool supplies a raw command
        // line; otherwise quoted the way Popen quoted an argument list.
        let command_line = match &spec.cmdline {
            Some(cmdline) => cmdline.clone(),
            None => {
                if spec.argv.is_empty() {
                    return Err("nothing to run".to_string());
                }
                list2cmdline(&spec.argv)
            }
        };
        let (child, reader) = process::start(&SpawnOptions {
            command_line,
            cwd: &spec.cwd,
            env: &spec.env,
            creation_flags: spec.creation_flags,
            pipe: spec.console,
        })
        .map_err(|e| e.to_string())?;

        let child = Arc::new(Mutex::new(child));
        self.child = Some(child.clone());
        let note = if label.is_empty() {
            spec.summary.clone()
        } else {
            label.to_string()
        };
        self.shared.set_state(RunState::Running, &note);

        let shared = self.shared.clone();
        let stop = self.stop.clone();
        let started_at = self.started_at;
        let on_started = spec.on_started.take();
        let handle = thread::Builder::new()
            .name("stress-runner".into())
            .spawn(move || {
                supervise(spec, on_started, child, reader, shared, stop, started_at);
            })
            .map_err(|e| e.to_string())?;
        self.thread = Some(handle);
        Ok(())
    }

    /// Ask the run to end. Safe to call when nothing is running.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(child) = &self.child {
            kill_tree(child);
        }
    }

    /// Wait for the supervising thread, up to *timeout* when given.
    pub fn wait(&self, timeout: Option<Duration>) {
        let deadline = timeout.map(|t| Instant::now() + t);
        while self.running() {
            if deadline.map(|d| Instant::now() >= d).unwrap_or(false) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Read the child's output and send whole lines.
///
/// Split on carriage returns as well as newlines: Linpack and y-cruncher
/// both redraw progress in place, and a reader that only knows about \n sits
/// on a growing buffer for minutes at a time.
fn pump_pipe(mut stream: std::io::PipeReader, sink: Sender<String>) {
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                for &byte in &chunk[..count] {
                    if byte == b'\n' || byte == b'\r' {
                        if !buffer.is_empty() {
                            let _ = sink.send(String::from_utf8_lossy(&buffer).into_owned());
                            buffer.clear();
                        }
                    } else {
                        buffer.push(byte);
                    }
                }
            }
        }
    }
    if !buffer.is_empty() {
        let _ = sink.send(String::from_utf8_lossy(&buffer).into_owned());
    }
}

/// What one line decided, if anything.
enum Verdict {
    None,
    Outcome(RunState, String, bool),
}

#[allow(clippy::too_many_arguments)]
fn supervise(
    spec: LaunchSpec,
    on_started: Option<crate::core::toolbase::OnStarted>,
    child: Arc<Mutex<Process>>,
    reader: Option<std::io::PipeReader>,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    started_at: Instant,
) {
    // The log baseline is taken first, before the tool is set up and before
    // it has written anything for this run. Taken afterwards it skipped
    // everything logged during startup -- including the block confirming the
    // settings the tool actually accepted, which is the one line worth having.
    let mut tails: Vec<FileTail> = spec
        .watch_files
        .iter()
        .cloned()
        .map(FileTail::new)
        .collect();
    let pid = child.lock().map(|c| c.id()).unwrap_or(0);

    // Tools that have to be driven through their own window are set up here:
    // on this thread, because waiting for a window to appear takes seconds.
    if let Some(setup) = on_started {
        match setup(pid) {
            Ok(detail) => {
                if !detail.is_empty() {
                    shared.emit(Event::Output(detail));
                }
            }
            Err(error) => {
                kill_tree(&child);
                shared.set_state(
                    RunState::Broken,
                    &format!("Could not set the tool up: {error}"),
                );
                return;
            }
        }
    }
    let deadline = (spec.duration_seconds > 0)
        .then(|| started_at + Duration::from_secs(spec.duration_seconds));

    // Whether this tool ever showed a window, and whether that window went
    // away while the process was still running. A window that vanishes first
    // is somebody closing it; a process that dies with its window still up
    // has crashed.
    let mut had_window = false;
    let mut window_closed_first = false;

    // Linpack's failure is a column in a table rather than a phrase, so its
    // rows are tracked here: the first residual becomes the reference every
    // later trial is compared against.
    let linpack = spec.error_key == "linpack";
    let mut reference_residual: Option<String> = None;
    let mut trials = 0u64;
    let (mut best, mut worst, mut total) = (0.0f64, 0.0f64, 0.0f64);

    let finished = errors::compile_plain(&spec.completion_patterns);
    let aborted = errors::compile_plain(&spec.abort_patterns);
    let setup_trouble = errors::compile_plain(&spec.setup_patterns);
    let mut saw_completion = false;

    let (line_sender, pipe_lines) = channel::<String>();
    let pump = reader.map(|stream| {
        thread::Builder::new()
            .name("stress-pipe".into())
            .spawn(move || pump_pipe(stream, line_sender))
            .ok()
    });
    let pump = pump.flatten();

    let judge = |stripped: &str| -> Verdict {
        if !setup_trouble.is_empty() && errors::matches(stripped, &setup_trouble).is_some() {
            return Verdict::Outcome(
                RunState::Broken,
                format!("The tool cannot run as configured: {stripped}"),
                false,
            );
        }
        if let Some(hit) = errors::scan(stripped, &spec.error_key) {
            return Verdict::Outcome(RunState::Failed, hit, false);
        }
        // Checked after the error patterns, so "the testing is completed, is
        // revealed 3 of errors!" is a failure rather than a completion.
        if !finished.is_empty() && errors::matches(stripped, &finished).is_some() {
            return Verdict::Outcome(
                RunState::Passed,
                format!("The tool reported it had finished: {stripped}"),
                true,
            );
        }
        if !aborted.is_empty() && errors::matches(stripped, &aborted).is_some() {
            return Verdict::Outcome(
                RunState::Stopped,
                format!("The tool stopped before finishing: {stripped}"),
                false,
            );
        }
        Verdict::None
    };

    let mut outcome: Option<RunState> = None;
    let mut note = String::new();
    let mut last_tick: Option<Instant> = None;

    'watch: loop {
        let mut lines: Vec<String> = Vec::new();
        // Drain whatever the pipe has produced without blocking, so the clock
        // and the log tails are still serviced on a quiet tool.
        while let Ok(line) = pipe_lines.try_recv() {
            lines.push(line);
        }
        for tail in tails.iter_mut() {
            let text = tail.read_new();
            if !text.is_empty() {
                lines.extend(splitlines(&text).into_iter().map(str::to_string));
            }
        }

        for line in &lines {
            let stripped = rstrip(line);
            if stripped.is_empty() {
                continue;
            }
            shared.emit(Event::Output(stripped.to_string()));

            if linpack {
                if let Some(row) = errors::scan_linpack_row(stripped) {
                    trials += 1;
                    total += row.gflops;
                    best = best.max(row.gflops);
                    worst = if trials == 1 {
                        row.gflops
                    } else {
                        worst.min(row.gflops)
                    };
                    shared.emit(Event::Stat(format!(
                        "trials {}  GFlops min {:.1} / avg {:.1} / max {:.1}",
                        trials,
                        worst,
                        total / trials as f64,
                        best
                    )));
                    if !row.passed {
                        let last = stripped.split_whitespace().last().unwrap_or("");
                        outcome = Some(RunState::Failed);
                        note = format!("Linpack check column reads '{last}' instead of 'pass'.");
                        break;
                    }
                    match &reference_residual {
                        None => reference_residual = Some(row.residual.clone()),
                        Some(reference) if *reference != row.residual => {
                            outcome = Some(RunState::Failed);
                            note = format!(
                                "Residual changed between identical trials: {reference} then {}.",
                                row.residual
                            );
                            break;
                        }
                        _ => {}
                    }
                    continue;
                }
            }

            if let Verdict::Outcome(state, text, completed) = judge(stripped) {
                outcome = Some(state);
                note = text;
                saw_completion = completed;
                break;
            }
        }

        if outcome.is_some() {
            break;
        }

        if stop.load(Ordering::SeqCst) {
            outcome = Some(RunState::Stopped);
            note = "Stopped.".into();
            break;
        }

        if !spec.console && !window_closed_first {
            let windows = winui::windows_of(pid);
            if !windows.is_empty() {
                had_window = true;
            } else if had_window && exited(&child).is_none() {
                window_closed_first = true;
            }
        }

        if let Some(code) = exited(&child) {
            // The last thing a tool writes is usually the thing worth
            // reading: its verdict. Drain the pipe, and re-read the log
            // files, before deciding what the exit meant.
            let mut leftovers: Vec<String> = Vec::new();
            if let Some(handle) = &pump {
                let until = Instant::now() + Duration::from_secs(3);
                while !handle.is_finished() && Instant::now() < until {
                    thread::sleep(Duration::from_millis(20));
                }
                while let Ok(line) = pipe_lines.try_recv() {
                    leftovers.push(line);
                }
            }
            for tail in tails.iter_mut() {
                let text = tail.read_new();
                if !text.is_empty() {
                    leftovers.extend(splitlines(&text).into_iter().map(str::to_string));
                }
            }

            for line in &leftovers {
                let stripped = rstrip(line);
                if stripped.is_empty() {
                    continue;
                }
                shared.emit(Event::Output(stripped.to_string()));
                if outcome.is_some() {
                    continue;
                }
                if let Verdict::Outcome(state, text, completed) = judge(stripped) {
                    outcome = Some(state);
                    note = text;
                    if completed {
                        saw_completion = true;
                    }
                }
            }

            if outcome.is_some() {
                break 'watch;
            }

            let (state, text) = if code == STATUS_CONTROL_C_EXIT {
                // Its console was closed, or it was sent Ctrl+C or
                // Ctrl+Break. That is somebody interrupting the test, not
                // memory or a core giving a wrong answer.
                (
                    RunState::Stopped,
                    "The tool's console was closed or interrupted (Ctrl+C). This is not a \
                     hardware failure, and the test did not finish."
                        .to_string(),
                )
            } else if code != 0 && window_closed_first {
                // A bad exit code, but the window went before the process
                // did -- so this was somebody closing it. The code describes
                // the teardown and not the machine.
                (
                    RunState::Stopped,
                    format!(
                        "The tool's window was closed, so the test did not run to the end. It \
                         exited with {}, which is what Windows gives a program taken down that \
                         way and says nothing about the machine.",
                        exit_code_text(code)
                    ),
                )
            } else if code != 0 {
                (
                    RunState::Failed,
                    format!(
                        "The tool exited with {}, which usually means it crashed.",
                        exit_code_text(code)
                    ),
                )
            } else if !finished.is_empty() && !saw_completion {
                // A clean exit from a tool that announces its own completion,
                // without that announcement. Closing the window does this,
                // and it is not a pass.
                (
                    RunState::Stopped,
                    "The tool exited without reporting that it had finished, so the test did \
                     not run to the end."
                        .to_string(),
                )
            } else {
                (
                    RunState::Passed,
                    "Finished on its own with no errors.".to_string(),
                )
            };
            outcome = Some(state);
            note = text;
            break;
        }

        let now = Instant::now();
        if let Some(limit) = deadline {
            if now >= limit {
                outcome = Some(RunState::Passed);
                note = "Reached the time limit with no errors.".into();
                break;
            }
        }

        if last_tick
            .map(|t| now.duration_since(t) >= Duration::from_secs(1))
            .unwrap_or(true)
        {
            last_tick = Some(now);
            shared.emit(Event::Tick {
                elapsed: now.duration_since(started_at).as_secs_f64(),
                remaining: deadline.map(|d| d.saturating_duration_since(now).as_secs_f64()),
            });
        }

        // Slept in slices so Stop is answered at once rather than at the end
        // of a two-second nap.
        let nap = Duration::from_secs_f64(if spec.console { 0.25 } else { POLL_SECONDS });
        let wake = Instant::now() + nap;
        while Instant::now() < wake && !stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(50));
        }
    }

    let outcome = outcome.unwrap_or(RunState::Stopped);

    // A tool that finished on its own and was asked to stay on screen is
    // left alone: it is sitting at its own "press any key" prompt with the
    // result showing, which is exactly what the user asked for. Anything else
    // is still running and has to be stopped.
    let left_open = spec.leave_open
        && outcome == RunState::Passed
        && saw_completion
        && exited(&child).is_none();
    if left_open {
        note.push_str(" Its window has been left open; close it when you have read the result.");
    } else {
        kill_tree(&child);
    }

    if let Ok(mut finding) = shared.finding.lock() {
        *finding = if outcome == RunState::Failed {
            note.clone()
        } else {
            String::new()
        };
    }
    shared.set_state(outcome, &note);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("roch-runner-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sniffing() {
        assert_eq!(FileTail::sniff(b"\xff\xfeh\0i\0"), Encoding::Utf16Le);
        assert_eq!(FileTail::sniff(b"\xfe\xff\0h\0i"), Encoding::Utf16Be);
        let le: Vec<u8> = "Running VSTv3: Passed"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(FileTail::sniff(&le), Encoding::Utf16Le);
        let be: Vec<u8> = "Running VSTv3: Passed"
            .encode_utf16()
            .flat_map(|u| u.to_be_bytes())
            .collect();
        assert_eq!(FileTail::sniff(&be), Encoding::Utf16Be);
        assert_eq!(FileTail::sniff(b"short\0"), Encoding::Utf8);
        assert_eq!(
            FileTail::sniff(b"\xef\xbb\xbfplain text that is long enough"),
            Encoding::Utf8
        );
    }

    #[test]
    fn tail_reads_only_what_is_appended() {
        let dir = temp("tail");
        let path = dir.join("log.txt");
        fs::write(&path, "before\n").unwrap();
        let mut tail = FileTail::new(path.clone());
        assert_eq!(tail.read_new(), "");
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        use std::io::Write;
        file.write_all(b"one\ntwo\n").unwrap();
        assert_eq!(tail.read_new(), "one\ntwo\n");
        assert_eq!(tail.read_new(), "");
        // Rewritten shorter: read again from the start.
        fs::write(&path, "x\n").unwrap();
        assert_eq!(tail.read_new(), "x\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_handles_utf16_split_across_reads_and_boms() {
        let dir = temp("utf16");
        let path = dir.join("log.txt");
        let mut tail = FileTail::new(path.clone());
        assert_eq!(tail.read_new(), "", "missing file is fine");
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("FAIL here\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()));
        // Write all but the last byte: the odd byte must be held back.
        fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        let first = tail.read_new();
        use std::io::Write;
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&bytes[bytes.len() - 1..]).unwrap();
        let second = tail.read_new();
        assert_eq!(format!("{first}{second}"), "FAIL here\r\n");

        let utf8 = dir.join("bom.txt");
        let mut tail = FileTail::new(utf8.clone());
        fs::write(&utf8, b"\xef\xbb\xbfTesting completed\n").unwrap();
        assert_eq!(tail.read_new(), "Testing completed\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn exit_codes_read_both_ways() {
        assert_eq!(
            exit_code_text(STATUS_ACCESS_VIOLATION),
            "code 3221225477 (0xC0000005)"
        );
        assert_eq!(exit_code_text(1), "code 1 (0x00000001)");
    }

    fn cmd_spec(script: &str, console: bool) -> LaunchSpec {
        LaunchSpec {
            argv: vec!["cmd".into(), "/d".into(), "/c".into(), script.into()],
            cwd: std::env::temp_dir(),
            console,
            error_key: "ycruncher".into(),
            summary: "test".into(),
            creation_flags: CREATE_NO_WINDOW,
            ..Default::default()
        }
    }

    fn run_to_end(spec: LaunchSpec) -> (RunState, String, Vec<Event>) {
        let mut runner = Runner::new();
        runner.start(spec, "label").expect("start");
        runner.wait(Some(Duration::from_secs(60)));
        let events = runner.drain();
        let (state, note) = events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::State { state, note } if *state != RunState::Running => {
                    Some((*state, note.clone()))
                }
                _ => None,
            })
            .expect("a final state");
        (state, note, events)
    }

    #[test]
    fn a_failure_line_on_the_pipe_fails_the_run() {
        let (state, note, events) = run_to_end(cmd_spec(
            "echo Running VSTv3: Passed & echo Running FFTv4: Failed",
            true,
        ));
        assert_eq!(state, RunState::Failed, "{note}");
        assert_eq!(note, "Running FFTv4: Failed");
        assert!(events.contains(&Event::Output("Running VSTv3: Passed".into())));
    }

    #[test]
    fn completion_patterns_decide_a_clean_exit() {
        let mut spec = cmd_spec("echo working", true);
        spec.completion_patterns = patterns(&["Test Finished"]);
        let (state, note, _) = run_to_end(spec);
        assert_eq!(state, RunState::Stopped, "{note}");

        let mut spec = cmd_spec("echo Test Finished", true);
        spec.completion_patterns = patterns(&["Test Finished"]);
        let (state, note, _) = run_to_end(spec);
        assert_eq!(state, RunState::Passed);
        assert!(note.starts_with("The tool reported it had finished"));

        let (state, note, _) = run_to_end(cmd_spec("echo fine", true));
        assert_eq!(state, RunState::Passed);
        assert_eq!(note, "Finished on its own with no errors.");
    }

    fn patterns(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn exit_codes_are_judged() {
        let (state, note, _) = run_to_end(cmd_spec("exit 3", true));
        assert_eq!(state, RunState::Failed);
        assert!(note.contains("code 3 (0x00000003)"), "{note}");
        let (state, _, _) = run_to_end(cmd_spec("exit -1073741510", true));
        assert_eq!(state, RunState::Stopped, "0xC000013A is an interruption");
    }

    #[test]
    fn log_files_are_watched() {
        let dir = temp("watch");
        let log = dir.join("results.txt");
        // A raw command line, because cmd's own quoting is not the C runtime's
        // and a quoted redirection target does not survive argv quoting. The
        // pause lets the tail take its baseline before anything is written.
        let mut spec = cmd_spec("", false);
        spec.cmdline = Some(format!(
            "cmd /d /c \"ping -n 2 127.0.0.1 >nul & echo FATAL ERROR: Rounding was 0.49> \"{}\"\"",
            log.display()
        ));
        spec.error_key = "prime95".into();
        spec.watch_files = vec![log];
        let (state, note, _) = run_to_end(spec);
        assert_eq!(state, RunState::Failed);
        assert_eq!(note, "FATAL ERROR: Rounding was 0.49");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn time_limit_and_stop_kill_the_process() {
        let mut spec = cmd_spec("ping -n 30 127.0.0.1 >nul", true);
        spec.duration_seconds = 1;
        let start = Instant::now();
        let (state, note, events) = run_to_end(spec);
        assert_eq!(state, RunState::Passed);
        assert_eq!(note, "Reached the time limit with no errors.");
        assert!(start.elapsed() < Duration::from_secs(20));
        assert!(events.iter().any(|e| matches!(e, Event::Tick { .. })));

        let mut runner = Runner::new();
        runner
            .start(cmd_spec("ping -n 30 127.0.0.1 >nul", true), "")
            .unwrap();
        assert!(runner.running());
        assert!(runner.start(cmd_spec("echo x", true), "").is_err());
        thread::sleep(Duration::from_millis(300));
        runner.stop();
        runner.wait(Some(Duration::from_secs(20)));
        assert!(!runner.running());
        assert_eq!(runner.state(), RunState::Stopped);
    }

    #[test]
    fn a_setup_step_that_fails_breaks_the_run() {
        let mut spec = cmd_spec("ping -n 30 127.0.0.1 >nul", true);
        spec.on_started = Some(Box::new(|pid| Err(format!("no window for {pid}"))));
        let (state, note, _) = run_to_end(spec);
        assert_eq!(state, RunState::Broken);
        assert!(note.starts_with("Could not set the tool up: no window for"));
    }

    #[test]
    fn linpack_rows_are_checked() {
        let script = "echo 1000 1000 4 0.01 50.2 1.2e-12 3.4e-02 pass & echo 1000 1000 4 0.01 51.0 1.3e-12 3.4e-02 pass";
        let mut spec = cmd_spec(script, true);
        spec.error_key = "linpack".into();
        let (state, note, events) = run_to_end(spec);
        assert_eq!(state, RunState::Failed);
        assert_eq!(
            note,
            "Residual changed between identical trials: 1.2e-12 then 1.3e-12."
        );
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Stat(s) if s.starts_with("trials 2"))));
    }
}
