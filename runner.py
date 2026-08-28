"""Starting a test, watching it, and deciding when it is over.

This is the part that makes the program worth more than five shortcuts. Every
tool is supervised the same way:

  * its output is collected -- from the pipe when it has one, from its log
    files when it does not,
  * every line is checked against that tool's failure patterns,
  * a wall-clock limit is enforced, because most of these tools cannot
    limit themselves,
  * and the process tree is killed properly when the run ends, whichever way
    it ends.

Everything here runs on a worker thread and reports back through a queue.
Nothing touches a widget: Tk is not thread-safe, and a stress test is exactly
the situation where a race would show up at hour four and never reproduce.
"""

import os
import queue
import subprocess
import threading
import time

import errors

IDLE = "idle"
RUNNING = "running"
PASSED = "passed"
FAILED = "failed"
STOPPED = "stopped"
BROKEN = "broken"

# 0xC000013A. Windows gives a console process this exit code when it is sent
# Ctrl+C or Ctrl+Break, or when its console window is closed. It says nothing
# about the health of the machine, so it must never be reported as a failure:
# a stress tester that cries hardware fault when somebody shut a window is
# worse than one that says nothing at all.
STATUS_CONTROL_C_EXIT = 0xC000013A

# How often the watcher wakes to poll log files and check the clock. Two
# seconds is far below any useful test duration and keeps the thread asleep
# almost all the time.
POLL_SECONDS = 2.0


class Event:
    """One thing that happened, on its way to the UI thread."""

    def __init__(self, kind, **data):
        self.kind = kind
        self.data = data

    def __repr__(self):
        return "Event(" + self.kind + ", " + repr(self.data) + ")"


class _FileTail:
    """Reads whatever gets appended to a file after we started watching.

    Files that do not exist yet are fine -- Prime95 creates results.txt only
    when it has something to say, which is the case we most want to catch.
    Files that shrink are treated as new files and re-read from the start,
    since that means the tool rotated or rewrote its log.
    """

    def __init__(self, path):
        self.path = path
        self.offset = os.path.getsize(path) if os.path.isfile(path) else 0
        # Decided from the byte-order mark on the first read. Not every log
        # here is UTF-8: TM5 writes UTF-8 with a BOM, and PowerShell's
        # Tee-Object -- which is how Linpack's output is captured while
        # staying visible in its own console -- writes UTF-16LE. Read as
        # UTF-8, a UTF-16 log decodes to text with a NUL between every
        # character, and not one pattern in errors.py matches it.
        self.encoding = None
        # A byte held back because a two-byte character straddled the end of
        # a read. Without this, one unlucky poll corrupts a line.
        self._pending = b""

    @staticmethod
    def _sniff(chunk):
        if chunk.startswith(b"\xff\xfe"):
            return "utf-16-le"
        if chunk.startswith(b"\xfe\xff"):
            return "utf-16-be"
        # No mark, but every second byte is a NUL: UTF-16 written without a
        # BOM. The test is deliberately strict -- a NUL at every odd position
        # across the first 32 bytes -- because guessing this one wrong would
        # garble an ordinary log, and a garbled log is a run whose failures
        # match nothing and are reported as a pass.
        head = chunk[:32]
        if len(head) >= 16 and all(byte == 0 for byte in head[1::2]):
            return "utf-16-le"
        if len(head) >= 16 and all(byte == 0 for byte in head[0::2]):
            return "utf-16-be"
        return "utf-8"

    def read_new(self):
        try:
            if not os.path.isfile(self.path):
                return ""
            size = os.path.getsize(self.path)
            if size < self.offset:
                # Rotated or rewritten: start again, and re-sniff, because it
                # may not be the same kind of file any more.
                self.offset = 0
                self.encoding = None
                self._pending = b""
            if size == self.offset:
                return ""
            with open(self.path, "rb") as handle:
                handle.seek(self.offset)
                chunk = handle.read()
            self.offset += len(chunk)

            data = self._pending + chunk
            self._pending = b""
            if self.encoding is None:
                self.encoding = self._sniff(data)
            if self.encoding.startswith("utf-16") and len(data) % 2:
                self._pending = data[-1:]
                data = data[:-1]
            return data.decode(self.encoding, errors="replace").lstrip("﻿")
        except OSError:
            return ""


def kill_tree(process):
    """Stop a process and anything it started.

    terminate() alone is not enough for the windowed tools, which ignore it,
    and Prime95 leaves worker threads mid-FFT if it is killed rudely without
    the tree being taken with it. taskkill /T /F is the one thing that
    reliably ends all of them.
    """
    if process is None or process.poll() is not None:
        return
    try:
        subprocess.run(
            ["taskkill", "/PID", str(process.pid), "/T", "/F"],
            capture_output=True,
            timeout=15,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
    except Exception:
        pass
    try:
        process.wait(timeout=10)
    except Exception:
        try:
            process.kill()
        except Exception:
            pass


class Runner:
    """Runs one LaunchSpec at a time and reports through ``events``.

    Event kinds:
      output  {"line"}          a line of the tool's output
      state   {"state", "note"} the run changed state
      tick    {"elapsed", ...}  once a second, for the status strip
      stat    {"text"}          a tool-specific running figure
    """

    def __init__(self):
        self.events = queue.Queue()
        self._process = None
        self._thread = None
        self._stop = threading.Event()
        self._lock = threading.Lock()
        self.state = IDLE
        self.spec = None
        self.started_at = 0.0
        self.finding = ""

    # -- control ---------------------------------------------------------

    @property
    def running(self):
        return self._thread is not None and self._thread.is_alive()

    def start(self, spec, label=""):
        """Launch *spec*. Raises OSError if the process will not start."""
        if self.running:
            raise RuntimeError("a test is already running")
        self._stop.clear()
        self.spec = spec
        self.finding = ""
        self.started_at = time.time()

        popen_args = {
            "cwd": spec.cwd,
            "env": spec.env,
            "creationflags": spec.creation_flags,
        }
        if spec.console:
            popen_args.update(
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                stdin=subprocess.DEVNULL,
                bufsize=0,
            )

        # A spec with a raw command line is handed to Windows verbatim; only
        # TestMem5 needs that, and only because of the shape of its arguments.
        self._process = subprocess.Popen(
            spec.cmdline if spec.cmdline else spec.argv, **popen_args
        )
        self._set_state(RUNNING, label or spec.summary)
        self._thread = threading.Thread(
            target=self._supervise, name="stress-runner", daemon=True
        )
        self._thread.start()

    def stop(self):
        """Ask the run to end. Safe to call when nothing is running."""
        self._stop.set()
        with self._lock:
            process = self._process
        kill_tree(process)

    def wait(self, timeout=None):
        if self._thread:
            self._thread.join(timeout)

    # -- internals -------------------------------------------------------

    def _emit(self, kind, **data):
        self.events.put(Event(kind, **data))

    def _set_state(self, state, note=""):
        self.state = state
        self._emit("state", state=state, note=note)

    def _supervise(self):
        spec = self.spec

        # The log baseline is taken first, before the tool is set up and
        # before it has written anything for this run. Taken afterwards it
        # skipped everything logged during startup -- including the block
        # confirming the settings the tool actually accepted, which is the
        # one line worth having.
        tails = [_FileTail(path) for path in spec.watch_files]

        # Tools that have to be driven through their own window are set up
        # here: on this thread, because waiting for a window to appear takes
        # seconds and the UI thread must not be blocked for them. A setup
        # that fails stops the run rather than letting it proceed with
        # settings nobody chose.
        if spec.on_started:
            try:
                # The adapter needs the process id to tell this tool's
                # message boxes apart from every other program's.
                spec.started_pid = self._process.pid
                detail = spec.on_started()
            except Exception as error:
                kill_tree(self._process)
                self._set_state(BROKEN,
                                "Could not set the tool up: " + str(error))
                return
            if detail:
                self._emit("output", line=detail)
        deadline = (
            self.started_at + spec.duration_seconds
            if spec.duration_seconds > 0
            else None
        )

        # Linpack's failure is a column in a table rather than a phrase, so
        # its rows are tracked here: the first residual becomes the reference
        # every later trial is compared against.
        linpack = spec.error_key == "linpack"
        reference_residual = None
        trials = 0
        best = worst = total = 0.0

        # A tool that announces it has finished rather than exiting. Read from
        # its output, because waiting on the process would wait for ever.
        finished = errors.compile_plain(spec.completion_patterns)
        aborted = errors.compile_plain(spec.abort_patterns)
        setup_trouble = errors.compile_plain(spec.setup_patterns)
        saw_completion = False

        reader = None
        pipe_lines = queue.Queue()
        if spec.console and self._process.stdout is not None:
            reader = threading.Thread(
                target=self._pump_pipe,
                args=(self._process.stdout, pipe_lines),
                name="stress-pipe",
                daemon=True,
            )
            reader.start()

        outcome = None
        note = ""
        last_tick = 0.0

        while True:
            lines = []

            # Drain whatever the pipe has produced without blocking, so the
            # clock and the log tails are still serviced on a quiet tool.
            while True:
                try:
                    lines.append(pipe_lines.get_nowait())
                except queue.Empty:
                    break

            for tail in tails:
                text = tail.read_new()
                if text:
                    lines.extend(text.splitlines())

            for line in lines:
                stripped = line.rstrip()
                if not stripped:
                    continue
                self._emit("output", line=stripped)

                if linpack:
                    row = errors.scan_linpack_row(stripped)
                    if row:
                        gflops, residual, passed = row
                        trials += 1
                        total += gflops
                        best = max(best, gflops)
                        worst = gflops if trials == 1 else min(worst, gflops)
                        self._emit(
                            "stat",
                            text="trials {}  GFlops min {:.1f} / avg {:.1f} / "
                                 "max {:.1f}".format(
                                     trials, worst, total / trials, best),
                        )
                        if not passed:
                            outcome, note = FAILED, (
                                "Linpack check column reads '" + stripped.split()[-1]
                                + "' instead of 'pass'.")
                            break
                        if reference_residual is None:
                            reference_residual = residual
                        elif residual != reference_residual:
                            outcome, note = FAILED, (
                                "Residual changed between identical trials: "
                                + reference_residual + " then " + residual + ".")
                            break
                        continue

                if setup_trouble and errors.matches(stripped, setup_trouble):
                    outcome, note = BROKEN, (
                        "The tool cannot run as configured: " + stripped)
                    break

                hit = errors.scan(stripped, spec.error_key)
                if hit:
                    outcome, note = FAILED, hit
                    break

                # Checked after the error patterns, so "the testing is
                # completed, is revealed 3 of errors!" is a failure rather
                # than a completion.
                if finished and errors.matches(stripped, finished):
                    outcome, note = PASSED, (
                        "The tool reported it had finished: " + stripped)
                    saw_completion = True
                    break
                if aborted and errors.matches(stripped, aborted):
                    outcome, note = STOPPED, (
                        "The tool stopped before finishing: " + stripped)
                    break

            if outcome:
                break

            if self._stop.is_set():
                outcome, note = STOPPED, "Stopped."
                break

            code = self._process.poll()
            if code is not None:
                # The last thing a tool writes is usually the thing worth
                # reading: its verdict. Drain the pipe, and re-read the log
                # files, before deciding what the exit meant -- a completion
                # line written a moment before exit used to be missed
                # entirely, and the exit judged on the code alone.
                leftovers = []
                if reader is not None:
                    reader.join(timeout=3)
                    while True:
                        try:
                            leftovers.append(pipe_lines.get_nowait())
                        except queue.Empty:
                            break
                for tail in tails:
                    text = tail.read_new()
                    if text:
                        leftovers.extend(text.splitlines())

                for line in leftovers:
                    stripped = line.rstrip()
                    if not stripped:
                        continue
                    self._emit("output", line=stripped)
                    if outcome:
                        continue
                    if setup_trouble and errors.matches(stripped, setup_trouble):
                        outcome, note = BROKEN, (
                            "The tool cannot run as configured: " + stripped)
                        continue
                    hit = errors.scan(stripped, spec.error_key)
                    if hit:
                        outcome, note = FAILED, hit
                    elif finished and errors.matches(stripped, finished):
                        outcome, note = PASSED, (
                            "The tool reported it had finished: " + stripped)
                        saw_completion = True
                    elif aborted and errors.matches(stripped, aborted):
                        outcome, note = STOPPED, (
                            "The tool stopped before finishing: " + stripped)

                if outcome:
                    break

                if code == STATUS_CONTROL_C_EXIT:
                    # Its console was closed, or it was sent Ctrl+C or
                    # Ctrl+Break. That is somebody or something interrupting
                    # the test, not memory or a core giving a wrong answer,
                    # and calling it a failure would have people chasing an
                    # instability that was never there.
                    outcome, note = STOPPED, (
                        "The tool's console was closed or interrupted "
                        "(Ctrl+C). This is not a hardware failure, and the "
                        "test did not finish.")
                elif code != 0:
                    outcome, note = FAILED, (
                        "The tool exited with code " + str(code)
                        + ", which usually means it crashed.")
                elif finished and not saw_completion:
                    # A clean exit from a tool that announces its own
                    # completion, without that announcement. Closing the
                    # window does this, and it is not a pass: nothing ran to
                    # the end.
                    outcome, note = STOPPED, (
                        "The tool exited without reporting that it had "
                        "finished, so the test did not run to the end.")
                else:
                    outcome, note = PASSED, "Finished on its own with no errors."
                break

            now = time.time()
            if deadline and now >= deadline:
                outcome, note = PASSED, "Reached the time limit with no errors."
                break

            if now - last_tick >= 1.0:
                last_tick = now
                self._emit(
                    "tick",
                    elapsed=now - self.started_at,
                    remaining=max(0.0, deadline - now) if deadline else None,
                )

            time.sleep(0.25 if spec.console else POLL_SECONDS)

        with self._lock:
            process = self._process

        # A tool that finished on its own and was asked to stay on screen is
        # left alone. It is not testing anything any more -- it is sitting at
        # its own "press any key" prompt with the result showing, which is
        # exactly what the user asked for by turning that option on. Anything
        # else is still running and has to be stopped.
        left_open = (
            spec.leave_open
            and outcome == PASSED
            and saw_completion
            and process is not None
            and process.poll() is None
        )
        if left_open:
            note += (" Its window has been left open; close it when you have "
                     "read the result.")
        else:
            kill_tree(process)

        self.finding = note if outcome == FAILED else ""
        self._set_state(outcome or STOPPED, note)

    @staticmethod
    def _pump_pipe(stream, sink):
        """Read the child's output and push whole lines onto *sink*.

        Split on carriage returns as well as newlines: Linpack and y-cruncher
        both redraw progress in place, and a reader that only knows about \\n
        sits on a growing buffer for minutes at a time.
        """
        buffer = b""
        try:
            while True:
                chunk = stream.read(1)
                if not chunk:
                    break
                if chunk in (b"\n", b"\r"):
                    if buffer:
                        sink.put(buffer.decode("utf-8", errors="replace"))
                        buffer = b""
                else:
                    buffer += chunk
        except Exception:
            pass
        finally:
            if buffer:
                sink.put(buffer.decode("utf-8", errors="replace"))
            try:
                stream.close()
            except Exception:
                pass
