"""y-cruncher's component stress tester.

The best-behaved tool of the set: a real command line, a real time limit, and
it prints to stdout, so the runner reads failures directly off the pipe rather
than hunting for a log. The whole adapter is one argument list.

The algorithm names come straight from Command Lines.txt in the distribution.
Naming any algorithm disables the rest, which is why "All algorithms" passes
none at all rather than listing them.
"""

import os

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
        "Output is read live from the process, so a failed comparison stops "
        "the run within a second of y-cruncher printing it."
    )

    fields = (
        Field("algorithms", "Algorithms", "text", "",
              hint="Blank runs all of them. Otherwise a space-separated "
                   "subset, e.g. VSTv3 FFTv4."),
        Field("memory", "Memory to use", "int", 0, minimum=0, maximum=1048576,
              unit="MB", hint="0 leaves y-cruncher's own default in place."),
        Field("per_test", "Seconds per test", "int", 60, minimum=5,
              maximum=86400, unit="s",
              hint="How long each algorithm runs before the next."),
        Field("duration", "Stop after", "int", 60, minimum=0, maximum=100000,
              unit="min",
              hint="Passed to y-cruncher as -TL. 0 runs until you press Stop."),
        Field("priority", "Process priority", "choice", "Normal",
              choices=["Below normal", "Normal", "Above normal", "High"],
              hint="y-cruncher defaults to below normal, which shares the "
                   "machine but under-stresses it."),
    )

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
        # pause:-2 is the one that matters for automation: without it a
        # finished or failed run leaves a console waiting on ENTER forever.
        argv = [
            exe,
            "skip-warnings",
            "pause:-2",
            "colors:0",
            f"priority:{self._PRIORITY.get(config.get('priority'), 0)}",
            f"logfile:{logfile}",
            "stress",
        ]

        memory = int(config.get("memory", 0))
        if memory > 0:
            argv.append(f"-M:{memory}M")

        per_test = int(config.get("per_test", 60))
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

        return LaunchSpec(
            argv=argv,
            cwd=os.path.dirname(exe),
            console=True,
            watch_files=[logfile],
            error_key=self.key,
            summary=(
                f"y-cruncher {' '.join(selected) if selected else 'all tests'}"
                f", {memory or 'default'} MB, {per_test}s per test"
            ),
            duration_seconds=duration_seconds,
            creation_flags=self._no_window_flags(),
        )
