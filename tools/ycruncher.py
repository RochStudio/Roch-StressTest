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
import re

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

# Every spelling y-cruncher accepts, mapped to the name offered here, so a
# hand-typed VT3 in a .bat is read back as the VSTv3 tick it means.
ALGORITHM_NAMES = {name.upper(): name for name, _ in ALGORITHMS}
ALGORITHM_NAMES.update({"VT3": "VSTv3", "VST": "VSTv3", "N63": "NTT63",
                        "N64": "NTT63", "SFT": "SFTv4", "FFT": "FFTv4"})

# Windows priority values from the manual's Startup Parameters section.
PRIORITIES = {
    "Below normal": -1,
    "Normal": 0,
    "Above normal": 1,
    "High": 2,
}

# What y-cruncher runs at when a .bat says nothing about priority, so a
# profile saved at this priority does not need to say it either.
NATIVE_PRIORITY = "Below normal"

# Characters Windows will not have in a file name.
_BAD_NAME = re.compile(r'[<>:"/\\|?*\x00-\x1f]')


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
    # No detection note: the tab is a profile editor and is kept to the
    # profile and its settings. For the record, shown in its
    # own window failures are read from y-cruncher's log file, which carries
    # the same text it prints; hidden, they are read straight off the
    # process. Either way a failed comparison stops the run within a couple
    # of seconds of y-cruncher printing it.

    fields = (
        Field("algorithms", "Algorithms", "multi", "VSTv3",
              choices=[name for name, _description in ALGORITHMS],
              hint="Tick as many as you want; they run one after another. "
                   "None ticked runs all of them, which is y-cruncher's own "
                   "default."),
        Field("memory", "Memory to use", "int", 0, minimum=0, maximum=1024,
              unit="GB", hint="-M. 0 leaves it out and y-cruncher picks."),
        Field("per_test", "Seconds per test", "int", 60, minimum=0,
              maximum=86400, unit="s",
              hint="-D. How long each algorithm runs before the next. 0 "
                   "leaves it out and y-cruncher picks."),
        Field("duration", "Stop after", "int", 60, minimum=0, maximum=100000,
              unit="min",
              hint="-TL, in seconds on the command line. y-cruncher checks "
                   "it between tests, so it stops at the first test boundary "
                   "at or after this; the countdown allows five minutes past "
                   "it before stepping in. 0 runs until you press Stop."),
        Field("pause", "Pause at the end", "bool", False,
              hint="pause:1. Keeps the finished result on screen."),
        Field("priority", "Process priority", "choice", "Normal",
              choices=list(PRIORITIES),
              hint="priority:. y-cruncher runs at below normal when a "
                   "profile does not say, which shares the machine but "
                   "under-stresses it."),
        Field("extra", "Other options", "text", "",
              hint="Anything else to pass, exactly as y-cruncher takes it. "
                   "Options before 'stress' in a .bat that this tab has no "
                   "box for are kept here too."),
        Field("show_window", "Show y-cruncher's window", "bool", True,
              hint="Only for Start on this tab; a saved .bat always shows "
                   "it. Failures are read from its log file either way."),
    )

    # What the Quick Start page runs, and what this tab opens on.
    quick_start = {
        "values": {"algorithms": "VSTv3", "memory": 28, "duration": 30,
                   "per_test": 0, "pause": True},
    }

    @staticmethod
    def _memory_argument(gigabytes):
        return "-M:" + str(gigabytes) + "GB"

    # -- profiles: the .bat files beside y-cruncher.exe ------------------

    def profiles(self, root):
        """(name, path) for each .bat beside y-cruncher.exe."""
        return [(os.path.splitext(os.path.basename(path))[0], path)
                for path in self.bat_files(root)]

    @staticmethod
    def profile_arguments(config):
        """The y-cruncher arguments these settings make, as a .bat holds them.

        Everything before "stress" is a startup parameter and everything
        after it belongs to the stress test, per the manual. Nothing is
        written that y-cruncher would do anyway, so a profile says only what
        was chosen.
        """
        startup = []
        if config.get("pause"):
            startup.append("pause:1")
        priority = config.get("priority", NATIVE_PRIORITY)
        if priority in PRIORITIES and priority != NATIVE_PRIORITY:
            startup.append("priority:" + str(PRIORITIES[priority]))

        test = []
        memory = int(config.get("memory", 0) or 0)
        if memory > 0:
            test.append(YCruncher._memory_argument(memory))
        per_test = int(config.get("per_test", 0) or 0)
        if per_test > 0:
            test.append("-D:" + str(per_test))
        minutes = int(config.get("duration", 0) or 0)
        if minutes > 0:
            test.append("-TL:" + str(minutes * 60))

        # Extras go on whichever side of "stress" they are shaped for: a
        # startup parameter is "name:value" with no leading dash.
        for token in str(config.get("extra", "")).split():
            if token.startswith("-") or ":" not in token:
                test.append(token)
            else:
                startup.append(token)

        chosen = str(config.get("algorithms", "")).replace(",", " ").split()
        selected = []
        for token in chosen:
            canonical = ALGORITHM_NAMES.get(token.upper())
            if canonical and canonical not in selected:
                selected.append(canonical)
        return startup + ["stress"] + test + selected

    profile_hint = ("Saved as <name>.bat beside y-cruncher.exe, and shown as "
                    "a button on Quick Start.")

    def profile_preview(self, config):
        return " ".join(["y-cruncher.exe"] + self.profile_arguments(config))

    @staticmethod
    def _gigabytes(text):
        """-M's value in whole gigabytes, or None when it cannot be read."""
        match = re.fullmatch(r"(\d+(?:\.\d+)?)([KMGT]?)(?:I?B)?", text.upper())
        if not match:
            return None
        scale = {"": 1024 ** -3, "K": 1024 ** -2, "M": 1024 ** -1,
                 "G": 1, "T": 1024}[match.group(2)]
        return max(1, round(float(match.group(1)) * scale))

    @classmethod
    def profile_values(cls, path):
        """A .bat read back into this tab's settings.

        Anything the tab has no box for is kept in "Other options" rather
        than dropped, so opening a profile and saving it again never loses
        part of it.
        """
        values = {"algorithms": "", "memory": 0, "per_test": 0,
                  "duration": 0, "pause": False,
                  "priority": NATIVE_PRIORITY, "extra": ""}
        extra, selected = [], []
        for token in cls._bat_arguments(path):
            lower = token.lower()
            name, _, value = token.partition(":")
            if lower == "stress":
                continue
            if lower.startswith("pause:"):
                values["pause"] = value.strip() == "1"
                if value.strip() not in ("1", "-2"):
                    extra.append(token)
                continue
            if lower.startswith("priority:"):
                for label, number in PRIORITIES.items():
                    if value.strip() == str(number):
                        values["priority"] = label
                        break
                else:
                    extra.append(token)
                continue
            if name.upper() in ("-M", "-D", "-TL"):
                number = (cls._gigabytes(value) if name.upper() == "-M"
                          else int(value) if value.isdigit() else None)
                if number is None:
                    extra.append(token)
                elif name.upper() == "-M":
                    values["memory"] = number
                elif name.upper() == "-D":
                    values["per_test"] = number
                else:
                    values["duration"] = max(1, round(number / 60))
                continue
            canonical = ALGORITHM_NAMES.get(token.upper())
            if canonical:
                if canonical not in selected:
                    selected.append(canonical)
                continue
            extra.append(token)
        values["algorithms"] = " ".join(selected)
        values["extra"] = " ".join(extra)
        return values

    @staticmethod
    def clean_profile_name(name):
        """A profile name that is safe to use as a file name, or ""."""
        return _BAD_NAME.sub("", str(name)).strip().strip(".")

    def save_profile(self, root, name, config):
        """Write the settings as <name>.bat beside y-cruncher.exe."""
        exe = self.locate(root)
        name = self.clean_profile_name(name)
        if not exe or not name:
            raise ToolUnavailable("A profile needs a name and y-cruncher.")
        path = os.path.join(os.path.dirname(exe), name + ".bat")
        line = " ".join(["y-cruncher.exe"] + self.profile_arguments(config))
        with open(path, "w", newline="\r\n") as handle:
            handle.write(line + "\n")
        return path

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

        # Exactly the command line a profile saved from this tab would hold,
        # so Start and the .bat it saves always run the same test. Added to
        # it, before "stress" where startup parameters belong:
        #
        # skip-warnings is the one that is not optional: without it y-cruncher
        # waits at a startup prompt for ENTER that nobody is there to press.
        #
        # pause:-2 when pause is off, so y-cruncher does not wait on its own
        # default; pause:1 is safe either way, because it exits immediately
        # once the child's input is closed -- measured at 6.3s against 6.4s
        # for pause:-2 on an identical run.
        arguments = self.profile_arguments(config)
        added = ["skip-warnings", "logfile:" + logfile]
        if not config.get("pause"):
            added.insert(0, "pause:-2")
        cut = arguments.index("stress")
        argv = [exe] + arguments[:cut] + added + arguments[cut:]

        memory = int(config.get("memory", 0) or 0)
        per_test = int(config.get("per_test", 0) or 0)
        duration_seconds = int(config.get("duration", 0) or 0) * 60
        selected = [a for a in arguments[cut:] if a in dict(ALGORITHMS)]

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
                f", {str(memory) + ' GB' if memory else 'default memory'}"
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
