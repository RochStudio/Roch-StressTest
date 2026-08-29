"""OCCT, opened at its own window.

OCCT is the one tool here that covers the whole machine from a single
program: an AVX2 CPU test, a memory test, a combined CPU+RAM run, a 3D
graphics load and a power-supply test that drives CPU and GPU together. Each
of them checks its own results and says so when they are wrong, which is the
same class of tool as Prime95 and Linpack rather than a benchmark.

It is opened rather than driven. Its command line exists -- `-t=cpu`,
`-d=<seconds>` and so on -- but the settings that matter are chosen very
differently per test type, and the free edition limits an unattended run to
an hour anyway. The window is where those choices are laid out, and the
error counters it shows are the reason to be looking at it.

Nothing is watched from here. OCCT keeps its own logs under Documents\\OCCT
and shows errors on screen as they happen; what the runner still does is hold
the run to a limit if one was set and notice if it dies.
"""

import os

from toolbase import Field, LaunchSpec, Tool, ToolUnavailable


class OCCT(Tool):
    key = "occt"
    name = "OCCT"
    blurb = (
        "CPU, memory, combined and power tests in one program, each checking "
        "its own answers. The AVX2 CPU test and the memory test are the two "
        "worth reaching for after a clock or timing change; the power test "
        "loads CPU and GPU together and is the hardest thing here on a PSU."
    )

    # A single portable executable, either beside this program or installed.
    exe_globs = ("OCCT*/OCCT*.exe", "OCCT*.exe")
    external_globs = (
        r"C:\Program Files\OCCT\OCCT*.exe",
        r"C:\Program Files (x86)\OCCT\OCCT*.exe",
    )

    # Configured in OCCT's own window, so there is no tab here.
    has_tab = False

    presets = ()

    fields = (
        Field("duration", "Stop after", "int", 0, minimum=0, maximum=100000,
              unit="min",
              hint="0 lets the test you set up in OCCT run to its own end."),
    )

    quick_start = {
        "values": {"duration": 0},
        "note": "Opens OCCT, where the test, its length and the error "
                "counters all live. Its own logs go to Documents\\OCCT.",
    }

    def quick_actions(self, root):
        """One button, and it opens the tool rather than starting a run."""
        return [("Open", self.quick_config(root))]

    def quick_summary(self, root):
        limit = int(self.quick_config(root).get("duration", 0) or 0)
        # Nothing worth a line of its own when there is no limit: the
        # button says "Open" and the note underneath says the rest.
        return str(limit) + " min" if limit else ""

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "OCCT.exe was not found. It is a single portable executable "
                "-- put it in an OCCT folder beside this program."
            )

        return LaunchSpec(
            argv=[exe],
            cwd=os.path.dirname(exe),
            console=False,
            error_key=self.key,
            summary="OCCT (test chosen in its own window)",
            duration_seconds=int(config.get("duration", 0)) * 60,
            leave_open=True,
        )
