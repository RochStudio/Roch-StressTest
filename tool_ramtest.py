"""RAM Test Pro, configured by filling in its window.

This is the one tool here with no way in. It has no command-line switches, no
settings file worth the name -- its settings.ini holds a window position and a
scale factor, nothing else -- and no registry keys. Thread count, memory size,
error limit and time limit exist only as four boxes in its window, and they
are not saved between runs. The .cfg files in config/ are the test algorithm
sequence and contain none of it.

So the adapter types into the window, through ``winui``, and reads every value
back before pressing Start. That is a brittle thing to do and it is done
defensively: a box that will not take its value stops the run being started at
all, because a memory test that quietly ran at the wrong size is worse than
one that did not run.

Two things are worth knowing about how it ends:

  * There is no "stop after N cycles" setting. The window has Max Errors and
    Max Time; "Cycle" is a counter it displays. A cycle limit is enforced from
    here instead, by watching its log for the start of the cycle after the
    last one wanted.
  * "ERROR! Free up RAM or reduce memory size for test!" is RAM Test Pro
    failing, not memory failing. Like TM5, it separates the two, and so does
    ``errors.RAMTEST``.
"""

import glob
import os
import time

import errors
import winui
from toolbase import Field, LaunchSpec, Preset, Tool, ToolUnavailable

WINDOW_TITLE = "RAM Test Pro"

# How long to wait for the window after starting the process. It is an 83 MB
# .NET executable and takes a few seconds on a cold start.
WINDOW_TIMEOUT_SECONDS = 40


class RamTestPro(Tool):
    key = "ramtest"
    name = "RAM Test Pro"
    blurb = (
        "A second opinion on memory, with a different test set from TM5. "
        "Worth running when TM5 passes but the machine still misbehaves."
    )
    exe_globs = (
        "ram_test_pro*/*/RAM Test Pro.exe",
        "ram_test_pro*/RAM Test Pro.exe",
        "RAMTestPro*/RAM Test Pro.exe",
        "RAM Test Pro.exe",
    )
    console = False
    detection_note = (
        "Its settings are typed into its window and read back before Start is "
        "pressed, so a run never starts with the wrong ones. Failures come "
        "from logs/log.txt: a non-zero 'Test errors detected' and its own "
        "ERROR lines. Running out of memory to test with is reported as a "
        "setup problem, not as instability, because that is what it is."
    )

    # The preset picker on the panel already offers exactly these, so
    # the panel hides this field rather than showing the same list twice.
    preset_field = "config"

    fields = (
        Field("config", "Configuration", "choice", "",
              hint="The .cfg files in RAM Test Pro's config folder."),
        Field("memory", "Memory to use", "int", 28000, minimum=0,
              maximum=1048576, unit="MB",
              hint="0 keeps whatever Auto worked out from free memory."),
        Field("auto_threads", "Auto-detect threads", "bool", True,
              hint="Presses the tool's own Auto button, which fills in the "
                   "thread count to match the machine."),
        Field("threads", "Threads", "int", 0, minimum=0, maximum=512,
              hint="Only used when Auto-detect is off. 0 leaves it alone."),
        Field("cycles", "Stop after", "int", 1, minimum=0, maximum=9999,
              unit="cycles",
              hint="RAM Test Pro has no cycle limit of its own, so this one "
                   "is enforced here by watching its log. 0 runs until the "
                   "time limit or Stop."),
        Field("max_errors", "Stop after", "int", 1, minimum=0, maximum=100000,
              unit="errors",
              hint="Its own Max Errors box. It stops itself when the count "
                   "reaches this."),
        Field("duration", "Stop after", "int", 0, minimum=0, maximum=100000,
              unit="min",
              hint="Its own Max Time box. 0 is no time limit."),
    )

    # What the Quick Start page runs, and what this tab opens on.
    quick_start = {
        "preset": "DDR4_DDR5_universal",
        "values": {"memory": 28000, "auto_threads": True, "cycles": 1,
                   "max_errors": 1, "duration": 0},
        "note": "28 GB, threads auto-detected, one cycle, stops on the first "
                "error.",
    }

    _NOTES = {
        "DDR4_DDR5_universal": "The one to use on any modern platform.",
        "DDR3_DDR2_universal": "For older machines.",
        "default": "The stock profile.",
    }

    def configs(self, root):
        """The .cfg files in the config folder, as (name, path) pairs."""
        exe = self.locate(root)
        if not exe:
            return []
        folder = os.path.join(os.path.dirname(exe), "config")
        found = []
        for path in sorted(glob.glob(os.path.join(folder, "*.cfg"))):
            found.append((os.path.splitext(os.path.basename(path))[0], path))
        # The DDR4/DDR5 profile first: it is the right answer on anything
        # this program is likely to be run on.
        found.sort(key=lambda pair: (not pair[0].startswith("DDR4"), pair[0]))
        return found

    def note_for(self, config_name):
        return self._NOTES.get(config_name, "")

    # -- driving the window ----------------------------------------------

    @staticmethod
    def _await_window(pid=None):
        deadline = time.time() + WINDOW_TIMEOUT_SECONDS
        while time.time() < deadline:
            hwnd = winui.find_window(WINDOW_TITLE, pid)
            if hwnd:
                # The controls exist a moment after the window does.
                time.sleep(1.0)
                return hwnd
            time.sleep(0.5)
        raise RuntimeError(
            "RAM Test Pro's window did not appear within "
            + str(WINDOW_TIMEOUT_SECONDS) + " seconds."
        )

    def _configure(self, config, pid=None):
        """Fill in the window, check it took, and press Start.

        Returns a line for the log describing what was actually set -- read
        back off the controls, not from what we meant to write.
        """
        hwnd = self._await_window(pid)
        found = winui.controls(hwnd)

        if config.get("auto_threads", True):
            auto = winui.button(found, "Auto")
            if auto is None:
                raise RuntimeError("RAM Test Pro has no Auto button.")
            winui.click(auto)
            # Auto fills in the thread count *and* a memory size of its own,
            # so anything we want to set has to be written after it.
            time.sleep(1.5)
            found = winui.controls(hwnd)

        wanted = []
        if not config.get("auto_threads", True) and int(config.get("threads", 0)):
            wanted.append(("Threads", int(config["threads"])))
        if int(config.get("memory", 0)):
            wanted.append(("Memory", int(config["memory"])))
        wanted.append(("Max Errors", int(config.get("max_errors", 0))))
        wanted.append(("Max Time", int(config.get("duration", 0))))

        for label, value in wanted:
            box = winui.box_beside(found, label)
            if box is None:
                raise RuntimeError(
                    "Could not find the '" + label + "' box in RAM Test "
                    "Pro's window. It may have changed in this version."
                )
            if not winui.set_text(box, value):
                raise RuntimeError(
                    "RAM Test Pro would not accept " + str(value) + " in '"
                    + label + "' -- it reads '" + winui.read_text(box) + "'."
                )

        start = winui.button(found, "Start")
        if start is None:
            raise RuntimeError("RAM Test Pro has no Start button.")

        # Read the boxes back one final time, so the log records what the
        # tool is actually about to run with rather than what it was told.
        settled = []
        for label in ("Threads", "Memory", "Max Errors", "Max Time"):
            box = winui.box_beside(found, label)
            if box is not None:
                settled.append(label + "=" + (winui.read_text(box) or "-"))

        winui.click(start)

        # It vets the settings when Start is pressed and answers a bad one
        # with a modal box -- "Memory block size must be at least 50 MB." is
        # the usual one, because it splits the memory across the threads and
        # each block has a floor. It then does not start. Left unnoticed, the
        # run reports nothing wrong and nothing happens.
        if pid:
            complaint = self._complaint(pid)
            if complaint:
                raise RuntimeError(
                    "RAM Test Pro refused these settings: " + complaint
                )

        return "Set in RAM Test Pro's window and started: " + ", ".join(settled)

    @staticmethod
    def _complaint(pid, wait=4.0):
        """Any message box the tool put up, dismissed and returned as text."""
        deadline = time.time() + wait
        while time.time() < deadline:
            box = winui.message_box(pid)
            if box:
                _hwnd, message, ok = box
                if ok is not None:
                    winui.click(ok)
                return message or "it showed a message box with no text."
            time.sleep(0.3)
        return ""

    # -- launching -------------------------------------------------------

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "'RAM Test Pro.exe' was not found. Expected a folder like "
                "ram_test_pro_1.5.0 beside this program."
            )

        folder = os.path.dirname(exe)
        available = dict(self.configs(root))
        chosen = str(config.get("config", "")).strip()
        if chosen not in available and available:
            chosen = next(iter(available))

        if chosen:
            # The file holds a bare filename, not a path -- that is the
            # format already in the box, and it is read relative to config/.
            self._write(
                os.path.join(folder, "config", "current_config.txt"),
                chosen + ".cfg",
            )

        # It logs the start of every cycle, so cycle N has finished when the
        # start of cycle N+1 appears. That is the only way to bound a run by
        # cycles: the tool itself offers errors and minutes, nothing else.
        cycles = int(config.get("cycles", 0))
        completion = []
        if cycles > 0:
            completion.append(r"Current Cycle\s+" + str(cycles + 1) + r"\b")

        settings = dict(config)
        summary_parts = [chosen or "default"]
        if int(config.get("memory", 0)):
            summary_parts.append(str(int(config["memory"])) + " MB")
        summary_parts.append("threads auto" if config.get("auto_threads")
                             else "threads " + str(config.get("threads") or "-"))
        if cycles:
            summary_parts.append(str(cycles) + " cycle"
                                 + ("s" if cycles != 1 else ""))
        if int(config.get("max_errors", 0)):
            summary_parts.append("stop at " + str(int(config["max_errors"]))
                                 + " error"
                                 + ("s" if int(config["max_errors"]) != 1 else ""))

        spec = LaunchSpec(
            argv=[exe],
            cwd=folder,
            console=False,
            watch_files=[os.path.join(folder, "logs", "log.txt")],
            error_key=self.key,
            summary="RAM Test Pro " + ", ".join(summary_parts),
            duration_seconds=int(config.get("duration", 0)) * 60,
            completion_patterns=completion,
            abort_patterns=errors.RAMTEST_ABORTED,
            # It stays up showing the error count and elapsed time once it
            # stops, which is the summary worth reading.
            leave_open=True,
        )
        # Set after construction so it can read the process id the runner
        # records on the spec a moment before calling it.
        spec.on_started = lambda: self._configure(settings, spec.started_pid)
        return spec

    def presets_for(self, root):
        made = [
            Preset(name, {"config": name}, self.note_for(name))
            for name, _ in self.configs(root)
        ]
        return tuple(made) or (Preset("None found", {}, ""),)
