"""y-cruncher's component stress tester.

The best-behaved tool of the set: a real command line, a real time limit, and
a logfile: option that mirrors everything it prints. The whole adapter is one
argument list.

That logfile is what lets it run in a visible console. A child whose stdout is
redirected leaves its own console blank, so a tool cannot be both piped and
watchable -- but y-cruncher will write the same text to a file, which the
runner tails instead. Nothing is lost by showing it.

The algorithm names come straight from Command Lines.txt in the distribution.
Naming any algorithm disables the rest, which is why "All algorithms" passes
none at all rather than listing them.
"""

import os

import errors
import hardware
import settings
from toolbase import Field, LaunchSpec, Preset, Tool, ToolUnavailable

# The valid values for [algorithm], in the order the manual lists them. N63,
# N64 and VST are aliases y-cruncher still accepts; only the canonical names
# are offered here so a saved queue does not depend on an alias surviving.
ALGORITHMS = (
    ("BKT", "Basecase + Karatsuba + Toom-Cook"),
    ("BBP", "Bailey-Borwein-Plouffe digit extraction"),
    ("SFTv4", "Small in-cache FFT"),
    ("SNT", "Small in-cache NTT"),
    ("SVT", "Small in-cache vector transform"),
    ("FFTv4", "Fast Fourier transform"),
    ("NTT63", "Classic 64-bit NTT"),
    ("VSTv3", "Vector-scalable transform"),
)


# How much longer than its own -TL y-cruncher is given before the runner
# steps in. It needs to be more than none: -TL is only checked between tests,
# so y-cruncher routinely runs past its limit to finish the test in progress,
# and a backstop that fired on the dot would kill it mid-test every time --
# taking the window, and the result printed in it, with it.
BACKSTOP_GRACE_SECONDS = 300


class YCruncher(Tool):
    key = "ycruncher"
    name = "y-cruncher"
    blurb = (
        "Component stress tester. VSTv3 and FFTv4 are the AVX-heavy pair that "
        "finds an unstable memory clock quickly; the small in-cache tests "
        "stay on the core."
    )
    exe_globs = ("y-cruncher*/y-cruncher.exe", "y-cruncher.exe")
    console = True
    detection_note = (
        "Shown in its own window, failures are read from y-cruncher's log "
        "file, which carries the same text it prints; hidden, they are read "
        "straight off the process. Either way a failed comparison stops the "
        "run within a couple of seconds of y-cruncher printing it."
    )

    fields = (
        Field("algorithms", "Algorithms", "text", "",
              hint="Blank runs all of them. Otherwise a space-separated "
                   "subset, e.g. VSTv3 FFTv4."),
        Field("memory", "Memory to use", "int", 0, minimum=0, maximum=1048576,
              unit="MB", hint="0 leaves y-cruncher's own default in place."),
        Field("per_test", "Seconds per test", "int", 60, minimum=0,
              maximum=86400, unit="s",
              hint="How long each algorithm runs before the next. 0 omits -D "
                   "and leaves y-cruncher's own default."),
        Field("show_window", "Show y-cruncher's window", "bool", True,
              hint="Runs it in its own console so you can watch it. Failures "
                   "are then read from its log file, which carries exactly "
                   "the same text."),
        Field("pause", "Pause at the end", "bool", False,
              hint="y-cruncher's pause:1. It leaves the finished result on "
                   "screen if the window is shown, and changes nothing if it "
                   "is not -- either way the process still exits."),
        Field("duration", "Stop after", "int", 60, minimum=0, maximum=100000,
              unit="min",
              hint="Passed to y-cruncher as -TL, which it checks between "
                   "tests -- so it stops at the first test boundary at or "
                   "after this, not on the dot. The countdown shown while it "
                   "runs allows five minutes past it before stepping in. "
                   "0 runs until you press Stop."),
        Field("priority", "Process priority", "choice", "Normal",
              choices=["Below normal", "Normal", "Above normal", "High"],
              hint="y-cruncher defaults to below normal, which shares the "
                   "machine but under-stresses it."),
    )

    # What the Quick Start page runs, and what this tab opens on.
    quick_start = {
        "preset": "VSTv3 only",
        "values": {"memory": 28 * 1024, "duration": 30, "per_test": 0,
                   "pause": True},
        "note": "VT3 alone, 28 GB, 30 minutes. More memory than is normally "
                "free, so Windows will page to reach it.",
    }

    presets = (
        Preset("All algorithms",
               {"algorithms": "", "per_test": 60},
               "The default sweep. Broadest coverage, slowest to repeat any "
               "one weakness."),
        Preset("Vector + FFT (VSTv3, FFTv4)",
               {"algorithms": "VSTv3 FFTv4", "per_test": 90},
               "The two that pull the most current and touch the most RAM. "
               "Start here for a memory or SoC voltage check."),
        Preset("VSTv3 only",
               {"algorithms": "VSTv3", "per_test": 120},
               "The single hottest test on AVX-512 parts."),
        Preset("In-cache only (SFTv4, SNT, SVT)",
               {"algorithms": "SFTv4 SNT SVT", "per_test": 60},
               "Stays inside the cache: a core and vcore test that mostly "
               "leaves the DIMMs alone."),
        Preset("Memory-heavy (FFTv4, NTT63, BKT)",
               {"algorithms": "FFTv4 NTT63 BKT", "per_test": 90},
               "Large working sets, for an IMC and DIMM check."),
        Preset("Custom", {}, "Whatever is in the boxes below."),
    )

    # Windows priority values from the manual's Startup Parameters section.
    _PRIORITY = {
        "Below normal": -1,
        "Normal": 0,
        "Above normal": 1,
        "High": 2,
    }

    @staticmethod
    def _memory_argument(megabytes):
        """-M in whole gigabytes when it divides evenly, megabytes otherwise.

        Cosmetic, and worth it: the command line is written to the log, and
        "-M:28GB" is the figure somebody typed, while "-M:28672M" is the same
        number after arithmetic they now have to redo to check it.
        """
        if megabytes % 1024 == 0:
            return "-M:" + str(megabytes // 1024) + "GB"
        return "-M:" + str(megabytes) + "M"

    def suggested_memory(self, preset_name):
        """Half of what is free, which is y-cruncher's own guidance.

        The stress tester allocates once and holds it. Taking everything free
        pushes Windows into the page file, and a run that is waiting on an SSD
        is not testing memory.
        """
        if preset_name.startswith("In-cache"):
            return 0
        return max(512, int(hardware.available_ram_mb() * 0.5))

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "y-cruncher.exe was not found. Expected a folder like "
                "'y-cruncher v0.8.7.9547b' beside this program."
            )

        work = settings.run_dir("ycruncher")
        logfile = os.path.join(work, "ycruncher.log")

        # Startup parameters come before the option, per the manual.
        #
        # skip-warnings is the one that is not optional: without it y-cruncher
        # waits at a startup prompt for ENTER that nobody is there to press.
        #
        # pause is the user's choice and safe either way. pause:1 prints
        # "Press any key to continue" and then exits immediately regardless,
        # because the child's input is closed; measured at 6.3s against 6.4s
        # for pause:-2 on an identical run.
        argv = [
            exe,
            "skip-warnings",
            "pause:1" if config.get("pause") else "pause:-2",
            "colors:0",
            f"priority:{self._PRIORITY.get(config.get('priority'), 0)}",
            f"logfile:{logfile}",
            "stress",
        ]

        memory = int(config.get("memory", 0))
        if memory > 0:
            argv.append(self._memory_argument(memory))

        # 0 omits -D entirely. That is not the same as a short one: -TL is
        # only checked between tests, so with no -D the run stops at the
        # first test boundary at or after the limit rather than on it. The
        # runner's own clock is what bounds the overshoot.
        per_test = int(config.get("per_test", 0))
        if per_test > 0:
            argv.append(f"-D:{per_test}")

        duration_seconds = int(config.get("duration", 0)) * 60
        if duration_seconds > 0:
            argv.append(f"-TL:{duration_seconds}")

        chosen = str(config.get("algorithms", "")).replace(",", " ").split()
        valid = {name.upper(): name for name, _ in ALGORITHMS}
        # Aliases the manual still documents, so a hand-typed VT3 works.
        valid.update({"VT3": "VSTv3", "N63": "NTT63", "N64": "NTT63",
                      "VST": "VSTv3"})
        selected = []
        for token in chosen:
            canonical = valid.get(token.upper())
            if canonical and canonical not in selected:
                selected.append(canonical)
        argv.extend(selected)

        try:
            if os.path.exists(logfile):
                os.remove(logfile)
        except OSError:
            pass

        # Shown in its own console, or piped and hidden -- never both. A
        # child whose stdout is redirected leaves its console blank, so the
        # visible case is watched through the log file instead. That file
        # carries the same text, which is exactly why watching both at once
        # used to print every line twice.
        show = bool(config.get("show_window", True))

        return LaunchSpec(
            argv=argv,
            cwd=os.path.dirname(exe),
            console=not show,
            watch_files=[logfile] if show else [],
            error_key=self.key,
            summary=(
                f"y-cruncher {' '.join(selected) if selected else 'all tests'}"
                f", {self._memory_argument(memory)[3:] if memory else 'default memory'}"
                + (f", {per_test}s per test" if per_test > 0
                   else ", default test length")
            ),
            # The backstop, not the limit. -TL above is what actually ends
            # the run; this only matters if y-cruncher never gets there.
            duration_seconds=(duration_seconds + BACKSTOP_GRACE_SECONDS
                              if duration_seconds else 0),
            creation_flags=(self._new_console_flags() if show
                            else self._no_window_flags()),
            # pause:1 holds the window open at the end -- verified, it sits
            # there indefinitely. That is only visible to anyone if the run
            # does not then kill it.
            leave_open=bool(show and config.get("pause")),
            # A clean exit without this line means the run ended early --
            # the window was closed, or the console was interrupted -- which
            # is not the same as passing.
            completion_patterns=errors.YCRUNCHER_COMPLETE,
        )
