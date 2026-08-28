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

import math
import os

import hardware
import settings
from toolbase import Field, LaunchSpec, Preset, Tool, ToolUnavailable


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

    def pick_binary(self, folder, found):
        """Which of the binaries in *folder* to run. Overridden by Xtreme."""
        return found

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                self.name + "'s binary was not found. Expected its folder "
                "beside this program."
            )

        blocked = self.unsupported_reason(root)
        if blocked:
            raise ToolUnavailable(blocked)

        folder = os.path.dirname(exe)
        exe = self.pick_binary(folder, exe)

        size = int(config.get("problem_size", 22528))
        lda = int(config.get("leading_dimension", 0)) or leading_dimension(size)
        if lda < size:
            lda = leading_dimension(size)
        alignment = int(config.get("alignment", 4))

        # Two banner lines, then: number of tests, size, leading dimension,
        # trials, alignment. 99999 trials is Linpack Extended's way of saying
        # "keep going"; the runner is what actually ends the test.
        work = settings.run_dir("linpack")
        input_path = os.path.join(work, "lininput")
        self._write(
            input_path,
            "Roch StressTest Linpack input\n"
            "Intel(R) Optimized LINPACK Benchmark data\n"
            "1\n"
            + str(size) + "\n"
            + str(lda) + "\n"
            "99999\n"
            + str(alignment) + "\n",
        )

        # Copied, never replaced: linpack.js hands the child a bare dict,
        # which drops PATH and stops the binary finding its OpenMP runtime.
        env = dict(os.environ)
        threads = int(config.get("threads", hardware.logical_cores()))
        env["OMP_NUM_THREADS"] = str(threads)
        env["MKL_NUM_THREADS"] = str(threads)

        if hardware.is_amd():
            # Without this the 2018 MKL build dies with an illegal
            # instruction on Zen 5 before printing a single result row --
            # its kernel dispatch picks a path the part does not implement.
            # Forcing the AVX2 code path fixes it, and is the same trick the
            # Linpack Xtreme front-end had to adopt to run on these CPUs.
            # MKL_ENABLE_INSTRUCTIONS=AVX2 is the documented spelling and
            # does *not* work here; only this one does.
            env["MKL_DEBUG_CPU_TYPE"] = "5"
        affinity = str(config.get("affinity", "")).strip()
        if affinity:
            env["KMP_AFFINITY"] = affinity
        else:
            env.pop("KMP_AFFINITY", None)

        # Linpack has no log option, so showing its console means teeing its
        # output: a redirected child leaves its own window blank, and the
        # pass/fail column exists nowhere but that output. PowerShell is the
        # only tee Windows ships. It writes UTF-16, which the runner's tail
        # reader detects from the byte-order mark.
        #
        # Paths are single-quoted for PowerShell, which is what carries the
        # spaces in "Roch StressTest" through intact.
        show = bool(config.get("show_window", True))
        tee_path = os.path.join(work, "linpack-output.txt")
        cmdline = None
        if show:
            try:
                if os.path.exists(tee_path):
                    os.remove(tee_path)
            except OSError:
                pass
            cmdline = (
                "powershell -NoProfile -ExecutionPolicy Bypass -Command "
                "\"& '" + exe + "' '" + input_path + "' | "
                "Tee-Object -FilePath '" + tee_path + "'\""
            )

        memory_gb = 8.0 * lda * size / (1024 ** 3)
        return LaunchSpec(
            argv=[exe, input_path],
            cmdline=cmdline,
            cwd=folder,
            env=env,
            console=not show,
            watch_files=[tee_path] if show else [],
            error_key=self.key,
            summary=(
                "Linpack " + os.path.basename(exe)
                + " n=" + str(size) + " lda=" + str(lda)
                + ", {:.1f} GB, ".format(memory_gb) + str(threads) + " threads"
            ),
            duration_seconds=int(config.get("duration", 0)) * 60,
            creation_flags=(self._new_console_flags() if show
                            else self._no_window_flags()),
        )


class LinpackXtreme(_Linpack):
    """Linpack Xtreme's binaries: the 2018.3 MKL build, in both vendor forms."""

    key = "linpack_xtreme"
    name = "Linpack Xtreme"
    blurb = (
        "The heaviest sustained AVX load of the set, and the quickest way to "
        "find a core or memory setting that is only nearly stable. This is "
        "the package to use on AMD -- it ships a build that runs there. "
        "Watch temperatures: nothing else here pulls this much current."
    )
    exe_globs = ("LinpackXtreme*/binaries/x64/linpack_*64.exe",)

    quick_start = {
        "preset": "4 GB",
        "values": {"duration": 30, "residual_check": True},
        "note": "4 GB problem, 30 minutes, residual checks on. Start here "
                "before the larger sizes.",
    }

    def pick_binary(self, folder, found):
        """The build that matches this processor.

        Xtreme ships both. The AMD one forces the AVX2 paths that the Intel
        one gates behind a vendor check, so running the Intel build on a
        Ryzen produces a number that means very little -- when it runs at all.
        """
        preferred = ("linpack_amd64.exe" if hardware.is_amd()
                     else "linpack_intel64.exe")
        candidate = os.path.join(folder, preferred)
        return candidate if os.path.isfile(candidate) else found


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
        "preset": "4 GB",
        "values": {"duration": 30, "residual_check": True},
        "note": "30 minutes with residual checks on. Intel only.",
    }
