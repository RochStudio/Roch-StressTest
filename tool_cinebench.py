"""Cinebench, in the six versions worth keeping around.

One tool with the version as its preset rather than six tabs, because that is
what the versions are: the same renderer, re-tuned, and picking R23 instead of
R20 is choosing a preset in every sense that matters here.

Two generations, two command lines, both confirmed by running them:

  R20 and later take named settings --

      Cinebench.exe g_CinebenchCpuXTest=true g_CinebenchMinimumTestDuration=1800

    and print "CB 24164.72 (0.00)" at the end. The duration is in seconds and
    is the thing that makes Cinebench a stress test at all: given one, it
    re-runs the render until that much time has passed, so a 30-minute
    setting is 30 minutes of sustained all-core load. Verified -- 45 seconds
    produced two passes where the default produced one.

  R15 is older and takes switches -- ``-cb_cpux`` for the all-core render.
    It has no duration setting, so it renders once and stops, however long
    that takes. That is a benchmark, not a soak, and the panel says so.

A word on what this is for. Cinebench is a benchmark: it reports a score, not
a verdict, and it has no idea whether the answer it computed was right. It
earns its place because a sustained all-core render is the load that finds an
unstable all-core clock or a cooling limit, and because the score falling
between runs is itself a signal -- but a pass here means "it finished", not
"the arithmetic was correct". Prime95 and y-cruncher are the tools that check
their own answers.
"""

import glob
import os
import re

from toolbase import Field, LaunchSpec, Preset, Tool, ToolUnavailable

# Where each version's executable is, in the order they should be searched.
# BenchMate keeps a version-numbered folder per release, which is why these
# end in a wildcard rather than a fixed number.
VERSIONS = (
    ("R15", ("CINEBENCH R15",), "CINEBENCH Windows 64 Bit.exe", "old"),
    ("R15 Extreme", ("CINEBENCH R15 EXTREME",),
     "CINEBENCH Windows 64 Bit.exe", "old"),
    ("R20", ("CINEBENCH R20",), "Cinebench.exe", "new"),
    ("R23", ("CINEBENCH R23",), "Cinebench.exe", "new"),
    ("R24", ("CINEBENCH 2024",), "Cinebench.exe", "new"),
    ("R26", ("CINEBENCH 2026",), "Cinebench.exe", "new"),
)

# Everywhere a Cinebench install turns up. BenchMate first because it is the
# one that keeps every version side by side.
SEARCH_ROOTS = (
    r"C:\Program Files (x86)\BenchMate*\apps",
    r"C:\Program Files\BenchMate*\apps",
    r"C:\Program Files\MAXON",
    r"C:\Program Files (x86)\MAXON",
)

# "CB 24164.72 (0.00)" -- the score, and the only line worth keeping.
SCORE = re.compile(r"^\s*CB\s+([\d.]+)", re.IGNORECASE)


class Cinebench(Tool):
    key = "cinebench"
    name = "Cinebench"
    blurb = (
        "A sustained all-core render: the load that finds an unstable all-core "
        "clock or a cooling limit. It reports a score rather than a verdict -- "
        "it does not check its own arithmetic the way Prime95 does -- so a "
        "pass here means it finished, and a score that drops between runs is "
        "worth as much as the number itself."
    )
    console = True
    detection_note = (
        "Cinebench has no notion of a wrong answer, so what is watched for is "
        "it failing to run: a driver or renderer that falls over mid-render. "
        "The score is read off the 'CB' line and reported when the run ends."
    )

    fields = (
        Field("version", "Version", "choice", "R23", choices=[],
              hint="Which Cinebench to run."),
        Field("test", "Test", "choice", "All cores",
              choices=["All cores", "Single core"],
              hint="All cores is the stress test; single core is a "
                   "clock-and-boost check."),
        Field("duration", "Stop after", "int", 30, minimum=0, maximum=100000,
              unit="min",
              hint="R20 and later re-run the render until this much time has "
                   "passed, which is what makes it a soak. R15 ignores it "
                   "and renders once."),
    )

    quick_start = {
        "preset": "R23",
        "values": {"test": "All cores", "duration": 30},
        "note": "R23, all cores, 30 minutes of sustained render.",
    }

    _NOTES = {
        "R15": "2013, and still the quickest comparison. One render, no "
               "duration setting.",
        "R15 Extreme": "R15's heavier scene. Also a single render.",
        "R20": "2019. Larger scene, and the first with a duration setting.",
        "R23": "2020, and the usual choice for a soak.",
        "R24": "2024. Adds a GPU test; this runs the CPU one.",
        "R26": "2026, the current release.",
    }

    # -- finding the versions --------------------------------------------

    @staticmethod
    def _find(folders, exe_name):
        for root in SEARCH_ROOTS:
            for folder in folders:
                pattern = os.path.join(root, folder, "*", exe_name)
                matches = [m for m in glob.glob(pattern) if os.path.isfile(m)]
                if matches:
                    return sorted(matches)[-1]
                pattern = os.path.join(root, folder, exe_name)
                matches = [m for m in glob.glob(pattern) if os.path.isfile(m)]
                if matches:
                    return sorted(matches)[-1]
        return None

    def installed(self, root):
        """Every Cinebench version present, as (label, path, generation)."""
        found = []
        for label, folders, exe_name, generation in VERSIONS:
            # Beside this program first, so a copy dropped in the folder wins
            # over an installed one.
            local = sorted(glob.glob(os.path.join(root, "Cinebench*" + label,
                                                  "**", exe_name),
                                     recursive=True))
            path = local[-1] if local else self._find(folders, exe_name)
            if path:
                found.append((label, path, generation))
        return found

    def locate(self, root):
        found = self.installed(root)
        return found[0][1] if found else None

    def note_for(self, label):
        return self._NOTES.get(label, "")

    def presets_for(self, root):
        made = [
            Preset(label, {"version": label}, self.note_for(label))
            for label, _path, _gen in self.installed(root)
        ]
        return tuple(made) or (Preset("None found", {}, ""),)

    # -- launching -------------------------------------------------------

    def build(self, config, root):
        found = {label: (path, gen) for label, path, gen in self.installed(root)}
        if not found:
            raise ToolUnavailable(
                "No Cinebench installation was found. Looked beside this "
                "program and under BenchMate and MAXON in Program Files."
            )

        wanted = str(config.get("version", "")).strip()
        if wanted not in found:
            wanted = next(iter(found))
        exe, generation = found[wanted]

        all_cores = str(config.get("test", "All cores")) == "All cores"
        minutes = int(config.get("duration", 0))

        if generation == "new":
            argv = [exe, "g_CinebenchCpuXTest=true" if all_cores
                    else "g_CinebenchCpu1Test=true"]
            if minutes > 0:
                # Seconds, despite the log line printing it in milliseconds.
                argv.append("g_CinebenchMinimumTestDuration="
                            + str(minutes * 60))
            held = minutes > 0
        else:
            # R15's switches. It renders once and stops; there is nothing to
            # pass it that would make it hold the load for longer.
            argv = [exe, "-cb_cpux" if all_cores else "-cb_cpu1"]
            held = False

        summary = "Cinebench " + wanted + ", " + (
            "all cores" if all_cores else "single core")
        summary += (", " + str(minutes) + " min" if held
                    else ", one render" if generation == "old"
                    else ", no time limit")

        return LaunchSpec(
            argv=argv,
            cwd=os.path.dirname(exe),
            console=True,
            error_key=self.key,
            summary=summary,
            # Cinebench ends by itself once its own duration is met, so the
            # runner's clock is a backstop with room for the render in
            # progress to finish rather than being cut off mid-frame.
            duration_seconds=(minutes * 60 + 300) if held else 0,
            creation_flags=self._no_window_flags(),
        )
