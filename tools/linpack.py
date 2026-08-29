"""Linpack, driven as the raw Intel MKL benchmark binary.

Both Linpack packages in this folder are front-ends around the same Intel
binary: Linpack Xtreme is an interactive console menu, Linpack Extended is a
Node script. Neither is scriptable in a way that survives being supervised, so
this adapter skips both front-ends and runs the benchmark binary directly --
the same one they run, with the input file written here.

That is also the only way to get honest error detection out of Linpack. It
does not stop or shout when a solve comes back wrong; it prints a table and
puts something other than "pass" in the last column, then carries on. Reading
that column is the whole reason for running it ourselves.

The input file format, and the trial count of 99999 that turns a fixed number
of runs into "until we stop it", are taken from the Node driver in
Linpack-Extended-master/dependencies/linpack.js.
"""

import json
import math
import os
import shutil

from core import hardware
from core import settings
from core.toolbase import Field, LaunchSpec, Preset, Tool, ToolUnavailable


TEE_JS = '// Written by Roch StressTest. cmd has no tee, and Linpack Extended has\n// no log option, so its output is split here: on to the console it is\n// running in, and into a file the runner can read. node is used because\n// the package already ships it -- adding a binary for this would be\n// worse, and PowerShell is what the console was moved away from.\nconst fs = require("fs");\nconst out = fs.createWriteStream(process.argv[2]);\nprocess.stdin.on("data", function (chunk) {\n  process.stdout.write(chunk);\n  out.write(chunk);\n});\nprocess.stdin.on("end", function () { out.end(); });\n'


def leading_dimension(problem_size, avx=True):
    """The leading dimension Intel's documentation asks for.

    From "leading dimensions.html" in the Linpack Extended package: best
    performance comes from the nearest odd multiple of 8 at or above the
    problem size -- 16 on AVX parts, meaning divisible by 16 but not by 32.
    Getting this wrong costs throughput rather than correctness, but a Linpack
    run that is 20% slow is 20% less stress.
    """
    step = 16 if avx else 8
    lda = ((problem_size + step - 1) // step) * step
    if lda % (step * 2) == 0:
        lda += step
    return lda


def problem_size_for(memory_mb, avx=True):
    """The largest problem that fits in *memory_mb*.

    The matrix alone needs 8 * lda * n bytes, so n starts at sqrt(bytes/8) and
    steps down until the padded leading dimension fits as well.
    """
    target = max(1, int(memory_mb)) * 1024 * 1024
    size = int(math.sqrt(target / 8.0))
    step = 16 if avx else 8
    size -= size % step
    while size > step:
        if 8 * leading_dimension(size, avx) * size <= target:
            return size
        size -= step
    return step


class _Linpack(Tool):
    """What both Linpack packages have in common, which is nearly everything.

    They are two front-ends over the same Intel benchmark. The input file,
    the leading-dimension arithmetic, the environment, the teed console and
    the row-by-row checking are identical; the binary and who is allowed to
    run it are not.
    """

    key = "linpack"
    name = "Linpack"
    blurb = (
        "The heaviest sustained AVX load of the set, and the quickest way to "
        "find a core or memory setting that is only nearly stable. Watch "
        "temperatures: nothing else here pulls this much current."
    )
    exe_globs = ()
    # Set on the build that refuses to run on anything but an Intel CPU.
    intel_only = False
    console = True
    detection_note = (
        "Every result row is parsed. A row whose check column is not 'pass', "
        "or whose residual drifts from the first trial, stops the test. "
        "Linpack itself would have carried on."
    )

    fields = (
        Field("memory", "Memory to use", "int", 4096, minimum=64,
              maximum=1048576, unit="MB",
              hint="Sets the problem size below whenever a preset is picked."),
        Field("problem_size", "Problem size", "int", 22528, minimum=1000,
              maximum=200000,
              hint="Number of equations. Larger means more RAM and a longer "
                   "trial."),
        Field("leading_dimension", "Leading dimension", "int", 22528,
              minimum=0, maximum=200000,
              hint="0 recomputes it from the problem size."),
        Field("alignment", "Alignment", "int", 4, minimum=0, maximum=64,
              unit="KB"),
        Field("threads", "Threads", "int", hardware.logical_cores(),
              minimum=1, maximum=512),
        Field("residual_check", "Stop on residual mismatch", "bool", True,
              hint="A residual that changes between identical trials is an "
                   "error even when the check column still says pass."),
        Field("duration", "Stop after", "int", 30, minimum=0, maximum=100000,
              unit="min", hint="0 runs until you press Stop."),
        Field("show_window", "Show Linpack's window", "bool", True,
              hint="Runs it in its own console so you can watch the GFlops "
                   "table fill in. Its output is copied to a file at the "
                   "same time, so nothing stops being checked."),
        Field("affinity", "KMP_AFFINITY", "text",
              "nowarnings,compact,1,0,granularity=fine",
              hint="Blank leaves the library to place threads itself."),
    )

    quick_start = {
        "preset": "4 GB",
        "values": {"duration": 30},
        "note": "4 GB problem, 30 minutes. Start here before the larger sizes.",
    }

    # The memory figures Linpack Xtreme's own menu offers, so a run started
    # here is comparable with one started from that front-end.
    presets = (
        Preset("2 GB", {"memory": 2048}, "Short trials, quick pass or fail."),
        Preset("4 GB", {"memory": 4096}, "The usual starting point."),
        Preset("6 GB", {"memory": 6144}, ""),
        Preset("8 GB", {"memory": 8192}, ""),
        # 11448 MB is not a round number for its own sake: it is the memory
        # figure whose problem size comes out at exactly 38736, with a leading
        # dimension of 38736 to match, which is the size Linpack Extended is
        # usually run at.
        Preset("11 GB", {"memory": 11448},
               "Problem size 38736. A long trial, and the size Linpack "
               "Extended is normally run at."),
        Preset("14 GB", {"memory": 14336},
               "Long trials. Leans on the DIMMs as hard as on the cores."),
        Preset("30 GB", {"memory": 30720},
               "Needs 32 GB installed and very little else running."),
        Preset("Custom", {}, "Whatever is in the boxes below."),
    )

    def unsupported_reason(self, root):
        """Why this build cannot run here, or "" when it can.

        Asked before anything is started, so a machine that cannot run a tool
        is told plainly instead of watching it exit cleanly having done
        nothing -- which is exactly what the Intel-only build does.
        """
        if self.intel_only and hardware.is_amd():
            return (
                "Linpack Extended ships the Intel-only build "
                "(linpack_xeon64.exe), which refuses to run on an AMD "
                "processor -- it prints \"runs on only genuine Intel "
                "processors\" and exits without testing anything. Use "
                "Linpack Xtreme on this machine; it ships an AMD build."
            )
        return ""

    def apply_memory(self, config):
        """Recompute problem size and leading dimension from the memory box.

        Called when a preset is chosen or the memory field is edited, so that
        the three stay consistent without the user doing the arithmetic.
        """
        size = problem_size_for(config.get("memory", 4096))
        config["problem_size"] = size
        config["leading_dimension"] = leading_dimension(size)
        return config

class LinpackXtreme(_Linpack):
    """Linpack Xtreme, opened at its own menu.

    The package is a console front-end around the Intel binary: it asks how
    much memory, how many trials and how long, then runs it and prints the
    table. Those questions are the whole configuration, and they are asked in
    a way that is far clearer than a tab of the same fields would be -- so it
    is opened and answered there rather than driven from here.
    """

    key = "linpack_xtreme"
    name = "Linpack Xtreme"
    blurb = (
        "The heaviest sustained AVX load of the set, and the quickest way to "
        "find a core or memory setting that is only nearly stable. This is "
        "the package to use on AMD -- it ships a build that runs there. "
        "Watch temperatures: nothing else here pulls this much current."
    )
    # The menu, not the benchmark binary underneath it. It picks the right
    # build for the processor itself, which is the other thing this used to
    # do by hand.
    exe_globs = ("LinpackXtreme*/LinpackXtreme_x64.exe",
                 "LinpackXtreme*/LinpackXtreme_x32.exe")

    # Answered in its own window, so there is no tab here.
    has_tab = False

    presets = ()

    fields = (
        Field("duration", "Stop after", "int", 0, minimum=0, maximum=100000,
              unit="min",
              hint="0 lets the trials you asked it for finish in their own "
                   "time."),
    )

    quick_start = {
        "values": {"duration": 0},
        "note": "Opens Linpack Xtreme at its menu, where it asks for memory, "
                "trials and time. Stop ends it.",
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
                "LinpackXtreme_x64.exe was not found. Expected its folder "
                "beside this program."
            )

        # Its own console, and nothing read from it. The front-end prints the
        # table to the screen and keeps no log, so there is no file to tail --
        # what the runner still does is hold it to a limit if one was set and
        # notice if it dies.
        return LaunchSpec(
            argv=[exe],
            cwd=os.path.dirname(exe),
            console=False,
            error_key=self.key,
            summary="Linpack Xtreme (settings answered at its menu)",
            duration_seconds=int(config.get("duration", 0)) * 60,
            leave_open=True,
            creation_flags=self._new_console_flags(),
        )


class LinpackExtended(_Linpack):
    """Linpack Extended's binary, which is Intel-only.

    Its linpack_xeon64.exe checks the vendor string and, on anything that is
    not an Intel processor, prints "This binary version of the SMP LINPACK
    benchmark is optimized for and runs on only genuine Intel processors" and
    exits with status zero. Nothing is tested and nothing looks wrong, which
    is why this tool refuses to start on AMD rather than letting it happen.
    """

    key = "linpack_extended"
    name = "Linpack Extended"
    blurb = (
        "The same Intel benchmark from the Linpack Extended package. Intel "
        "processors only -- its binary refuses to run anywhere else. On AMD, "
        "use Linpack Xtreme instead."
    )
    exe_globs = ("Linpack-Extended*/dependencies/linpack/linpack_xeon64.exe",)
    intel_only = True

    quick_start = {
        "preset": "11 GB",
        # KMP_AFFINITY blank on purpose. linpack.js only overrides the child's
        # environment when it is set -- and when it does, it replaces the
        # environment rather than adding to it, so the thread count never
        # arrives and the default placement gives one thread per physical
        # core: 8 of 16 on this kind of part, half the load the tab asked for.
        # Left blank, the binary inherits the environment below and the
        # Threads field means something again. It is also this package's own
        # documented answer to an OMP error at startup.
        "values": {"duration": 30, "residual_check": True, "affinity": "",
                   "alignment": 1},
        "note": "Problem size 38736 for 30 minutes, residual checks on, "
                "alignment 1. Intel only.",
    }

    def build(self, config, root):
        """Run the package the way the package runs itself.

        Linpack Extended is a Node driver around the same Intel binary, and it
        is worth using rather than going around: it writes the input file,
        chains tests, keeps Min/Avg/Max GFlops per problem size, and stops on
        a bad solve or a residual that moved -- printing "FAIL - severe
        instability detected" or "RESIDUAL MISMATCH - instability detected",
        which are already the patterns in errors.LINPACK.

        Its settings live in config.json at the root of the package, and the
        path is hard-coded in linpack.js, so that file is where the settings
        have to go. The one that shipped is copied aside the first time rather
        than being written over and lost.
        """
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                self.name + "'s binary was not found. Expected its folder "
                "beside this program."
            )
        blocked = self.unsupported_reason(root)
        if blocked:
            raise ToolUnavailable(blocked)

        # .../dependencies/linpack/linpack_xeon64.exe
        dependencies = os.path.dirname(os.path.dirname(exe))
        package = os.path.dirname(dependencies)
        node = os.path.join(dependencies, "node", "node.exe")
        driver = os.path.join(dependencies, "linpack.js")
        for needed, what in ((node, "node.exe"), (driver, "linpack.js")):
            if not os.path.isfile(needed):
                raise ToolUnavailable(
                    "Linpack Extended is incomplete: " + what + " is missing "
                    "from its dependencies folder."
                )

        size = int(config.get("problem_size", 22528))
        lda = int(config.get("leading_dimension", 0)) or leading_dimension(size)
        if lda < size:
            lda = leading_dimension(size)
        alignment = int(config.get("alignment", 4))

        # linpack.js moves to the next test when the minutes are up, and the
        # runner is what ends the run. 0 here means "until stopped", so the
        # test is given a length nothing will reach.
        minutes = int(config.get("duration", 0)) or 100000

        settings_path = os.path.join(package, "config.json")
        original = settings_path + ".roch-original"
        try:
            if os.path.exists(settings_path) and not os.path.exists(original):
                shutil.copyfile(settings_path, original)
        except OSError:
            pass

        # linpack.js reads this as `config.settings.KMP_AFFINITY ?? ""`, so
        # leaving the key out is the same as an empty one -- and it is what
        # the configurations people actually pass around look like.
        block = {
            "reduce output below X problem size": 0,
            "track stats below X problem size": 0,
            "stop after residual mismatch":
                bool(config.get("residual_check", True)),
        }
        affinity = str(config.get("affinity", "")).strip()
        if affinity:
            block["KMP_AFFINITY"] = affinity

        self._write(settings_path, json.dumps({
            "test order": [1],
            "settings": block,
            "tests": {
                "1": {
                    "minutes": minutes,
                    "problem size": size,
                    "leading dimension": lda,
                    "alignment value": alignment,
                },
            },
        }, indent=2) + chr(10))

        work = settings.run_dir("linpack")
        log = os.path.join(work, "linpack-extended-output.txt")
        try:
            if os.path.exists(log):
                os.remove(log)
        except OSError:
            pass

        env = dict(os.environ)
        # What "Linpack Extended.bat" sets. node here is old enough that it
        # refuses to start on a recent Windows build without it.
        env["NODE_SKIP_PLATFORM_CHECK"] = "1"

        # These reach the benchmark only while KMP_AFFINITY is blank, because
        # that is the one case where linpack.js leaves the child's environment
        # alone. Set anyway: harmless when they are ignored, and the whole
        # point of the field when they are not.
        threads = int(config.get("threads", hardware.logical_cores()))
        env["OMP_NUM_THREADS"] = str(threads)
        env["MKL_NUM_THREADS"] = str(threads)

        memory_gb = 8.0 * lda * size / (1024 ** 3)
        summary = ("Linpack Extended n=" + str(size) + " lda=" + str(lda)
                   + ", {:.1f} GB, ".format(memory_gb) + str(threads)
                   + " threads")
        seconds = int(config.get("duration", 0)) * 60
        complete = ["All tests successfully passed"]

        if not bool(config.get("show_window", True)):
            return LaunchSpec(
                argv=[node, driver],
                cwd=dependencies,
                env=env,
                console=True,
                error_key=self.key,
                summary=summary,
                duration_seconds=seconds,
                completion_patterns=complete,
                creation_flags=self._no_window_flags(),
            )

        # cmd, as the package's own .bat uses -- but cmd has no tee, and the
        # driver has no log option, so its output would either be on screen or
        # readable and never both. node is already here, so node splits it.
        tee = os.path.join(work, "tee.js")
        self._write(tee, TEE_JS)
        cmdline = (
            'cmd /c ""' + node + '" "' + driver + '" 2>&1 | '
            '"' + node + '" "' + tee + '" "' + log + '""'
        )
        return LaunchSpec(
            argv=[node, driver],
            cmdline=cmdline,
            cwd=dependencies,
            env=env,
            console=False,
            watch_files=[log],
            error_key=self.key,
            summary=summary,
            duration_seconds=seconds,
            completion_patterns=complete,
            creation_flags=self._new_console_flags(),
        )
