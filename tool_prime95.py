"""Prime95's torture test, driven from its own configuration file.

Prime95 has exactly two command-line switches worth anything here: ``-t`` to
start the torture test and ``-W`` to point it at a working directory. Every
setting the torture dialog offers is a key in ``prime.txt`` in that directory,
so the adapter writes that file and hands over.

Most references, including Prime95's own undoc.txt, put the torture keys in
``local.txt``. In 30.19 that is a migration path rather than where they live:
give it a local.txt and it folds those keys into prime.txt, deletes local.txt,
and carries on. Writing prime.txt directly skips a step that only ever half
happened, and it is verifiable -- ask for MinTortureFFT=MaxTortureFFT=1024 and
results.txt says "Self-test 1024K passed!" and nothing else.

Pointing ``-W`` at our own scratch directory rather than the distribution
folder matters more than it looks. Prime95 writes prime.txt, results.txt and
worker save files wherever it is told to work; sending them to LOCALAPPDATA
leaves the unpacked p95 folder byte-identical to what was downloaded, and
gives results.txt a stable path to watch for failures.
"""

import os

import hardware
import settings
from toolbase import Field, LaunchSpec, Preset, Tool, ToolUnavailable


class Prime95(Tool):
    key = "prime95"
    name = "Prime95"
    blurb = (
        "FFT torture test. Smallest and Small FFTs are the heat and power "
        "test; Large, Blend and Huge lean on the memory controller and RAM."
    )
    exe_globs = ("p95*/prime95.exe", "prime95*/prime95.exe", "prime95.exe")
    console = False
    detection_note = (
        "Failures are read from results.txt, which Prime95 writes on every "
        "error, so a worker that fails while you are away is still caught."
    )

    fields = (
        Field("min_fft", "Min FFT size", "int", 4, minimum=4, maximum=65536,
              unit="K", hint="Smallest transform in the sweep."),
        Field("max_fft", "Max FFT size", "int", 32, minimum=4, maximum=65536,
              unit="K", hint="Largest transform in the sweep."),
        Field("memory", "Memory to use", "int", 0, minimum=0, maximum=1048576,
              unit="MB",
              hint="0 runs in place, which keeps the test inside cache."),
        Field("time_per_fft", "Minutes per FFT", "int", 6, minimum=1,
              maximum=600, unit="min",
              hint="How long each transform size is held before moving on."),
        Field("cores", "Cores to test", "int", hardware.physical_cores(),
              minimum=1, maximum=512),
        Field("hyperthreading", "Use hyperthreading / SMT", "bool",
              hardware.has_smt(),
              hint="Runs two threads per core, as the torture dialog does."),
        Field("alternate_in_place", "Alternate in-place and RAM", "bool", True,
              hint="Prime95's own default when a run is given memory to use."),
        Field("duration", "Stop after", "int", 60, minimum=0, maximum=100000,
              unit="min",
              hint="The only way a torture test ends. Prime95 has no cycle "
                   "count: it walks the FFT range, minutes-per-FFT at each "
                   "size, then starts again. 0 runs until you press Stop."),
        Field("extra", "Extra prime.txt lines", "text", "",
              hint="Passed through verbatim, e.g. TortureWeak=1048576."),
    )

    # What the Quick Start page runs, and what this tab opens on.
    quick_start = {
        "preset": "Large FFTs (2048K-8192K)",
        "values": {"duration": 30},
        "note": "Memory controller and RAM, 30 minutes. Prime95 has no cycle "
                "count -- it sweeps the FFT range and starts over until the "
                "time is up.",
    }

    # The four the torture dialog offers, plus one for DDR5 boards where the
    # interesting failures live above 8192K, and Custom for everything else.
    presets = (
        Preset("Smallest FFTs (4K-32K)",
               {"min_fft": 4, "max_fft": 32, "memory": 0, "time_per_fft": 4},
               "Stays in L1/L2. The heat and vcore test, and the one that "
               "fails an unstable core clock fastest."),
        Preset("Small FFTs (32K-1024K)",
               {"min_fft": 32, "max_fft": 1024, "memory": 0,
                "time_per_fft": 6},
               "L1 through L3, maximum sustained power draw."),
        Preset("Large FFTs (2048K-8192K)",
               {"min_fft": 2048, "max_fft": 8192, "time_per_fft": 6},
               "Leans on the memory controller with real RAM in play."),
        Preset("Blend (4K-8192K)",
               {"min_fft": 4, "max_fft": 8192, "time_per_fft": 6},
               "Some of everything, and the most RAM of the four."),
        Preset("Huge FFTs (8192K-32768K)",
               {"min_fft": 8192, "max_fft": 32768, "time_per_fft": 10},
               "Past the cache entirely: an IMC and DIMM test on DDR5."),
        Preset("Custom", {}, "Whatever is in the boxes below."),
    )

    def suggested_memory(self, preset_name):
        """The memory figure a preset should start with, in MB.

        Prime95 asks for a number, not a share, so this reads the machine at
        the moment the preset is picked. A quarter of what is free leaves
        Windows and the browser you forgot to close enough room that the run
        tests the DIMMs rather than the page file.
        """
        if preset_name.startswith(("Smallest", "Small ")):
            return 0
        free = hardware.available_ram_mb()
        if preset_name.startswith("Large"):
            return max(1024, int(free * 0.25))
        if preset_name.startswith(("Blend", "Huge")):
            return max(1024, int(free * 0.5))
        return 0

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "prime95.exe was not found. Expected a folder like "
                "p95v3019b20.win64 beside this program."
            )

        work = settings.run_dir("prime95")

        min_fft = int(config.get("min_fft", 4))
        max_fft = int(config.get("max_fft", 32))
        if max_fft < min_fft:
            min_fft, max_fft = max_fft, min_fft

        # Everything goes in prime.txt. The torture settings are documented as
        # living in local.txt, and 30.19 does still read them from there -- but
        # what it then does is migrate them into prime.txt and delete
        # local.txt, so writing local.txt means writing a file that exists for
        # about a second. Written here directly it is one file, no migration
        # step, and the settings survive being read back.
        #
        # The first two keys are what stop the GIMPS welcome dialog appearing
        # and waiting for a human: StressTester=1 is the "just stress testing"
        # answer. The two Converted/version keys mark the file as already
        # migrated, which suppresses the version-upgrade prompt.
        lines = [
            "StressTester=1",
            "UsePrimenet=0",
            "V24OptionsConverted=1",
            "WGUID_version=2",
            f"MinTortureFFT={min_fft}",
            f"MaxTortureFFT={max_fft}",
            f"TortureMem={int(config.get('memory', 0))}",
            f"TortureTime={int(config.get('time_per_fft', 6))}",
            f"TortureCores={int(config.get('cores', hardware.physical_cores()))}",
            f"TortureHyperthreading={1 if config.get('hyperthreading') else 0}",
            "TortureAlternateInPlace="
            f"{1 if config.get('alternate_in_place', True) else 0}",
        ]
        extra = str(config.get("extra", "")).strip()
        if extra:
            lines.extend(part.strip() for part in extra.splitlines() if part.strip())
        self._write(os.path.join(work, "prime.txt"), "\n".join(lines) + "\n")

        # A local.txt left over from an earlier version of this program would
        # be migrated on top of what was just written, silently reinstating
        # the previous run's settings.
        try:
            stale = os.path.join(work, "local.txt")
            if os.path.exists(stale):
                os.remove(stale)
        except OSError:
            pass

        # Started fresh each run so the failure watcher never reports an
        # error left over from the last one.
        results = os.path.join(work, "results.txt")
        try:
            if os.path.exists(results):
                os.remove(results)
        except OSError:
            pass

        smt = " + SMT" if config.get("hyperthreading") else ""
        return LaunchSpec(
            argv=[exe, f"-W{work}", "-t"],
            cwd=os.path.dirname(exe),
            console=False,
            watch_files=[results],
            error_key=self.key,
            summary=(
                f"Prime95 {min_fft}K-{max_fft}K, "
                f"{config.get('memory', 0)} MB, "
                f"{config.get('cores')} cores{smt}"
            ),
            duration_seconds=int(config.get("duration", 0)) * 60,
        )
