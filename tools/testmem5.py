"""TestMem5, opened at its own window.

Everything that makes one TM5 profile different from another -- which tests
run, in what order, for how many cycles, how large the testing window is --
lives in a .cfg in its bin folder, and picking one *is* the configuration.
That is why the profile is chosen in TM5's own window rather than here.

It was chosen here once, and the cycle count with it. TM5 keeps that count in
the file rather than on the command line, so overriding it meant writing a
copy of the profile -- which TM5 then displayed as its configuration, "Roch
active" instead of the name that was picked, and a run given two cycles was
seen going into a third. Its documented parameters are only Config File and
Minutes, and Minutes is "test at least": a floor, not a limit.

What is kept is the part worth having. TM5 0.13.1 appends to Log.txt beside
TM5.exe -- the configuration it loaded, the memory it took, every error, and
how the run ended -- so a failure at 3am is caught however the run was set
up. The patterns in errors.TESTMEM5 are TM5's own message templates, read out
of the strings in TM5.exe and TM5.dll rather than guessed, including the ones
that must *not* count: TM5 separates a failure of the memory under test from
a failure of TM5 itself and says outright, "This is not a failure of tested
memory."
"""

import os

from core import errors

from core.toolbase import Field, LaunchSpec, Tool, ToolUnavailable

# The one file this adapter writes into TM5's bin folder, used only when
# a cycle count overrides the profile's own.
WORKING_NAME = "Roch active"
WORKING_CFG = WORKING_NAME + ".cfg"


class TestMem5(Tool):
    key = "testmem5"
    name = "TestMem5"
    blurb = (
        "The memory test overclockers actually use. The anta777 profiles are "
        "the ones worth running: Extreme for a first pass, Absolut when you "
        "want to be sure. Pick the profile and the cycles in TM5's own "
        "window."
    )
    exe_globs = ("TestMem5*/TM5.exe", "TM5.exe")

    # Configured in TM5's own window, so there is no tab here.
    #
    # It used to be driven from one: the profile was chosen here and the
    # cycle count written into a copy of the .cfg, because TM5 keeps its
    # cycle count in the file rather than on the command line. That copy is
    # what TM5 then showed as its configuration -- "Roch active" instead of
    # the profile you picked -- and a run given two cycles was seen going
    # into a third. Its documented parameters are only Config File and
    # Minutes, and Minutes is "test at least", a floor rather than a limit.
    # Choosing the profile in the window that owns it avoids all of that.
    has_tab = False

    presets = ()

    fields = (
        Field("duration", "Stop after", "int", 0, minimum=0, maximum=100000,
              unit="min",
              hint="0 lets the cycles you set in TM5 run to their end."),
    )

    quick_start = {
        "values": {"duration": 0},
    }

    detection_note = (
        "TM5 0.13.1 appends to Log.txt beside TM5.exe, and that is read "
        "live: 'Error in test #N', a non-zero error total, and a crash "
        "blamed on the tested memory all stop the run."
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
                "TM5.exe was not found. Expected a TestMem5 folder beside "
                "this program."
            )

        folder = os.path.dirname(exe)
        log = os.path.join(folder, "Log.txt")

        # Started fresh so the watcher never reports an error left from the
        # last run. TM5 appends, so without this every run inherits the one
        # before it.
        try:
            if os.path.exists(log):
                os.remove(log)
        except OSError:
            pass

        # No arguments at all. Given a Config File it starts that profile
        # immediately; given none it opens and waits, which is the point.
        return LaunchSpec(
            argv=[exe],
            cwd=folder,
            console=False,
            watch_files=[log],
            error_key=self.key,
            summary="TestMem5 (profile and cycles set in its own window)",
            duration_seconds=int(config.get("duration", 0)) * 60,
            completion_patterns=errors.TESTMEM5_COMPLETE,
            abort_patterns=errors.TESTMEM5_ABORTED,
            leave_open=True,
        )
