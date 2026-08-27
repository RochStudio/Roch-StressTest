"""Starting a test, watching it, and deciding when it is over.

This is the part that makes the program worth more than five shortcuts. Every
tool is supervised the same way:

  * its output is collected -- from the pipe when it has one, from its log
    files when it does not,
  * every line is checked against that tool's failure patterns,
  * a wall-clock limit is enforced, because only y-cruncher and Linpack can
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

    def read_new(self):
        try:
            if not os.path.isfile(self.path):
                return ""
            size = os.path.getsize(self.path)
            if size < self.offset:
                self.offset = 0
            if size == self.offset:
                return ""
            with open(self.path, "rb") as handle:
                handle.seek(self.offset)
                chunk = handle.read()
            self.offset += len(chunk)
            return chunk.decode("utf-8", errors="replace")
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
        tails = [_FileTail(path) for path in spec.watch_files]
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
                # Give the reader a moment to hand over the tail of the pipe
                # before deciding what the exit meant.
                if reader is not None:
                    reader.join(timeout=3)
                    while True:
                        try:
                            leftover = pipe_lines.get_nowait().rstrip()
                        except queue.Empty:
                            break
                        if leftover:
                            self._emit("output", line=leftover)
                            hit = errors.scan(leftover, spec.error_key)
                            if hit:
                                outcome, note = FAILED, hit
                if outcome:
                    break
                if code == 0:
                    outcome, note = PASSED, "Finished on its own with no errors."
                else:
                    outcome, note = FAILED, (
                        "The tool exited with code " + str(code)
                        + ", which usually means it crashed.")
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


class Sequence:
    """Runs a list of steps one after another, stopping on the first failure.

    A step is ``(tool, config, label)``. This is what makes a stability run
    rather than a stress test: an hour of TM5, then half an hour of
    y-cruncher, then Linpack, unattended, with the first failure ending it and
    saying which step failed.
    """

    def __init__(self, runner):
        self.runner = runner
        self.steps = []
        self.index = -1
        self._thread = None
        self._abort = threading.Event()
        self.results = []

    @property
    def running(self):
        return self._thread is not None and self._thread.is_alive()

    def start(self, steps, root):
        if self.running:
            raise RuntimeError("the queue is already running")
        self.steps = list(steps)
        self.results = []
        self.index = -1
        self._abort.clear()
        self._thread = threading.Thread(
            target=self._run_all, args=(root,), name="stress-queue", daemon=True
        )
        self._thread.start()

    def stop(self):
        self._abort.set()
        self.runner.stop()

    def _run_all(self, root):
        for index, (tool, config, label) in enumerate(self.steps):
            if self._abort.is_set():
                break
            self.index = index
            self.runner._emit(
                "step",
                index=index,
                total=len(self.steps),
                label=label,
                state=RUNNING,
            )
            try:
                spec = tool.build(dict(config), root)
                self.runner.start(spec, label)
            except Exception as error:
                self.results.append((label, BROKEN, str(error)))
                self.runner._emit(
                    "step", index=index, total=len(self.steps),
                    label=label, state=BROKEN, note=str(error),
                )
                break

            self.runner.wait()
            state = self.runner.state
            self.results.append((label, state, self.runner.finding))
            self.runner._emit(
                "step", index=index, total=len(self.steps), label=label,
                state=state, note=self.runner.finding,
            )
            # A failure ends the queue: everything after it would be testing a
            # machine already known to be unstable.
            if state in (FAILED, BROKEN, STOPPED):
                break

        self.index = -1
        self.runner._emit("queue-done", results=list(self.results))
