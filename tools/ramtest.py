"""RAM Test Pro, opened at its own window.

This is the one tool here with no way in. It has no command-line switches, no
settings file worth the name -- its settings.ini holds a window position and a
scale factor, nothing else -- and no registry keys. Thread count, memory size,
error limit and time limit exist only as four boxes in its window.

Those boxes used to be filled in from here, through ``winui``, with every
value read back before Start was pressed. It worked, and it was brittle in
the way that typing into somebody else's window always is. It also meant this
program deciding settings that the window lays out more clearly than a tab
ever did, and refusing to start a run when it could not.

Its errors are still read either way. logs/log.txt carries a non-zero "Test
errors detected" and its own ERROR lines, and the one that reads like a
failure and is not -- "ERROR! Free up RAM..." -- is RAM Test Pro failing to
get the memory it was asked for, which is a setting to fix rather than a DIMM
to blame.
"""

import os

from core import errors
from core.toolbase import Field, LaunchSpec, Tool, ToolUnavailable

WINDOW_TITLE = "RAM Test Pro"

# How long to wait for the window after starting the process. It is an 83 MB
# .NET executable and takes a few seconds on a cold start.
WINDOW_TIMEOUT_SECONDS = 40


class RamTestPro(Tool):
    key = "ramtest"
    name = "RAM Test Pro"
    blurb = (
        "A commercial memory test with its own error counters and a stop-on-"
        "first-error option. Set the profile, the size and the thread count "
        "in its own window."
    )
    exe_globs = ("ram_test_pro*/**/RAM Test Pro.exe", "**/RAM Test Pro.exe")

    # Configured in its own window, so there is no tab here. It used to be
    # driven from one -- the fields typed in and read back before Start was
    # pressed, because RAM Test Pro has no command line at all -- and that
    # worked, but it meant this program deciding settings the window lays out
    # far more clearly, and refusing a run when it could not.
    has_tab = False

    presets = ()

    fields = (
        Field("duration", "Stop after", "int", 0, minimum=0, maximum=100000,
              unit="min",
              hint="0 lets the run you started in RAM Test Pro finish."),
    )

    quick_start = {
        "values": {"duration": 0},
    }

    detection_note = (
        "Failures are read from logs/log.txt: a non-zero 'Test errors "
        "detected' and its own ERROR lines."
    )

    def quick_actions(self, root):
        """One button, and it opens the tool rather than starting a run."""
        return [("Open", self.quick_config(root))]

    def quick_summary(self, root):
        limit = int(self.quick_config(root).get("duration", 0) or 0)
        return str(limit) + " min" if limit else ""

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "RAM Test Pro.exe was not found. Expected its folder beside "
                "this program."
            )

        folder = os.path.dirname(exe)
        log = os.path.join(folder, "logs", "log.txt")

        # The log itself is not committed -- it is a log -- and git does not
        # keep empty directories, so a fresh clone has no logs folder at all.
        # RAM Test Pro is not asked to cope with that: the errors this reads
        # are the whole reason for watching it.
        try:
            os.makedirs(os.path.dirname(log), exist_ok=True)
        except OSError:
            pass

        try:
            if os.path.exists(log):
                os.remove(log)
        except OSError:
            pass

        return LaunchSpec(
            argv=[exe],
            cwd=folder,
            console=False,
            watch_files=[log],
            error_key=self.key,
            summary="RAM Test Pro (settings entered in its own window)",
            duration_seconds=int(config.get("duration", 0)) * 60,
            # No completion pattern. The old one was built from the cycle
            # count this program had just typed in -- "Current Cycle N+1" --
            # and that count is now RAM Test Pro's own business. Its errors
            # are still read, and its "Test stopped by user" still says the
            # run ended rather than passed.
            abort_patterns=errors.RAMTEST_ABORTED,
            leave_open=True,
        )
