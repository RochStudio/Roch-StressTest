"""y-cruncher's component stress tester.

The best-behaved tool of the set: a real command line, a real time limit, and
a logfile: option that mirrors everything it prints. The whole adapter is one
argument list.

That logfile is what lets it run in a visible console. A child whose stdout is
redirected leaves its own console blank, so a tool cannot be both piped and
watchable -- but y-cruncher will write the same text to a file, which the
runner tails instead. Nothing is lost by showing it.

The algorithm names come straight from Command Lines.txt in the distribution.
Naming any algorithm disables the rest, which is why ticking none of them
passes none at all rather than listing them: that is how y-cruncher is told
to run the lot.
"""

import glob
import os

from core import errors
from core import settings
from core.toolbase import Field, LaunchSpec, Tool, ToolUnavailable

# The valid values for [algorithm], in the order the manual lists them. N63,
# N64 and VST are aliases y-cruncher still accepts; only the canonical names
# are offered here, so nothing saved depends on an alias surviving.
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
        Field("algorithms", "Algorithms", "multi", "VSTv3",
              choices=[name for name, _description in ALGORITHMS],
              hint="Tick as many as you want; they run one after another. "
                   "None ticked runs all of them, which is y-cruncher's own "
                   "default."),
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
        "values": {"algorithms": "VSTv3", "memory": 28 * 1024, "duration": 30,
                   "per_test": 0, "pause": True},
        "note": "VT3 alone, 28 GB, 30 minutes. More memory than is normally "
                "free, so Windows will page to reach it.",
    }

    def quick_summary(self, root):
        """Named by what is ticked, since there is no preset to name."""
        config = self.quick_config(root)
        chosen = str(config.get("algorithms", "")).split()
        parts = [" ".join(chosen) if chosen else "all algorithms"]
        if int(config.get("memory", 0) or 0):
            parts.append(self._memory_argument(int(config["memory"]))[3:])
        minutes = int(config.get("duration", 0) or 0)
        parts.append(str(minutes) + " min" if minutes else "no time limit")
        return "  |  ".join(parts)

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

    def bat_files(self, root):
        """Every .bat sitting beside y-cruncher.exe, sorted by name.

        Read from the folder rather than listed here, so a .bat dropped in
        beside it gets a button without this file being touched -- the same
        way TestMem5 and RAM Test Pro take their profiles from what is on
        disk.
        """
        exe = self.locate(root)
        if not exe:
            return []
        found = glob.glob(os.path.join(os.path.dirname(exe), "*.bat"))
        return sorted(found, key=lambda p: os.path.basename(p).lower())

    def quick_actions(self, root):
        """Open the menu, or run one of the .bat files beside it."""
        actions = [("Open", dict(self.quick_config(root), quick_mode="open"))]
        for bat in self.bat_files(root):
            label = os.path.splitext(os.path.basename(bat))[0].upper()
            actions.append((label, dict(self.quick_config(root),
                                        quick_mode="bat", bat_path=bat)))
        return actions

    def quick_summary(self, root):
        names = [os.path.splitext(os.path.basename(b))[0].upper()
                 for b in self.bat_files(root)]
        if not names:
            return super().quick_summary(root)
        return "Open the menu, or run " + " / ".join(names)

    @staticmethod
    def _bat_arguments(path):
        """The y-cruncher arguments out of a .bat, or [] if there are none.

        These files are one line calling y-cruncher.exe, which is all this
        needs to understand. Anything more elaborate is left alone rather
        than half-read.
        """
        try:
            with open(path, "r", errors="replace") as handle:
                text = handle.read()
        except OSError:
            return []
        for line in text.splitlines():
            stripped = line.strip()
            if "y-cruncher" in stripped.lower() and ".exe" in stripped.lower():
                parts = stripped.split()
                for index, part in enumerate(parts):
                    if part.lower().endswith("y-cruncher.exe") \
                            or part.lower().endswith('y-cruncher.exe"'):
                        return parts[index + 1:]
        return []

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "y-cruncher.exe was not found. Expected a folder like "
                "'y-cruncher v0.8.7.9547b' beside this program."
            )

        work = settings.run_dir("ycruncher")
        mode = str(config.get("quick_mode", ""))

        if mode == "open":
            # No arguments at all: y-cruncher's own menu, which is what it
            # does when it is double-clicked. Nothing is watched because
            # nothing has been chosen yet -- this is the button for going and
            # looking rather than for starting a run.
            return LaunchSpec(
                argv=[exe],
                cwd=os.path.dirname(exe),
                console=False,
                error_key=self.key,
                summary="y-cruncher (its own menu)",
                duration_seconds=0,
                leave_open=True,
                creation_flags=self._new_console_flags(),
            )

        logfile = os.path.join(work, "ycruncher.log")

        if mode == "bat":
            bat = str(config.get("bat_path", ""))
            arguments = self._bat_arguments(bat)
            if not arguments:
                raise ToolUnavailable(
                    os.path.basename(bat) + " has no y-cruncher command in "
                    "it that this could read."
                )

            # The .bat decides the test. Three things are added to what it
            # says, and nothing is taken away: logfile: is the only reason a
            # run in a visible console can be watched at all, and
            # skip-warnings stops it waiting at a startup prompt for an ENTER
            # nobody is there to press. Startup parameters have to come
            # before "stress", per the manual, so they are inserted rather
            # than appended.
            #
            # colors:0 is deliberately NOT added. It was, on the assumption
            # that colour would put escape codes in the log -- it does not.
            # A run logged with colour left on contains no 0x1b byte at all:
            # the colour is applied to the console, and the logfile is
            # written plain either way. All it did was take the colour off
            # the window somebody is watching.
            added = ["skip-warnings", "logfile:" + logfile]
            if "stress" in arguments:
                cut = arguments.index("stress")
                argv = [exe] + arguments[:cut] + added + arguments[cut:]
            else:
                argv = [exe] + added + arguments

            # -TL: is y-cruncher's own limit. It checks it only between tests
            # and routinely overruns, so the runner's clock allows five
            # minutes past it before stepping in -- the same grace the normal
            # path uses.
            seconds = 0
            for part in arguments:
                if part.lower().startswith("-tl:"):
                    try:
                        seconds = int(part.split(":", 1)[1]) + 300
                    except ValueError:
                        seconds = 0

            try:
                if os.path.exists(logfile):
                    os.remove(logfile)
            except OSError:
                pass

            return LaunchSpec(
                argv=argv,
                cwd=os.path.dirname(exe),
                console=False,
                watch_files=[logfile],
                error_key=self.key,
                summary=("y-cruncher " + os.path.basename(bat) + ": "
                         + " ".join(arguments)),
                duration_seconds=seconds,
                leave_open=True,
                creation_flags=self._new_console_flags(),
            )

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
