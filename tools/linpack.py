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

import glob
import json
import math
import os
import re
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

    # A config is a whole config.json -- the chain of tests and the settings
    # block -- kept as a file in the package's profiles folder, in exactly
    # the format linpack.js reads. The tab edits them and Quick Start has a
    # button for each, the way y-cruncher does with its .bat files.
    #
    # The tests are not Fields: there can be any number of them, so the tab
    # draws them itself and they travel in the config as "tests", a list in
    # the order they run.
    presets = ()
    fields = (
        Field("residual_check", "Stop on residual mismatch", "bool", False,
              hint="\"stop after residual mismatch\". A residual that changes "
                   "between identical trials is an error even when the check "
                   "column still says pass."),
        Field("affinity", "KMP_AFFINITY", "text", "",
              hint="Blank leaves it out, and is the package's own answer to "
                   "an OMP error at startup. It is also the only way the "
                   "Threads setting below reaches the benchmark: when it is "
                   "set, linpack.js replaces the environment with it."),
        Field("reduce_below", "Reduce output below", "int", 0, minimum=0,
              maximum=200000, unit="n",
              hint="\"reduce output below X problem size\". Quieter console "
                   "for small tests. 0 is off."),
        Field("track_below", "Track stats below", "int", 0, minimum=0,
              maximum=200000, unit="n",
              hint="\"track stats below X problem size\". Min/Avg/Max GFlops "
                   "for tests under this size. 0 is off."),
        Field("threads", "Threads", "int", hardware.logical_cores(),
              minimum=1, maximum=512,
              hint="Not part of the config. Only used while KMP_AFFINITY is "
                   "blank."),
        Field("show_window", "Show Linpack's window", "bool", True,
              hint="Not part of the config. Its output is copied to a file "
                   "at the same time, so nothing stops being checked."),
    )

    # The config Quick Start's summary and this tab open on, when it exists.
    QUICK_PROFILE = "11GB"
    # Always first on Quick Start: the package's own config.
    DEFAULT_PROFILE = "Default"

    TEST_DEFAULTS = {"minutes": 30, "problem size": 38736,
                     "leading dimension": 38736, "alignment value": 4}

    # -- configs: the .json files in the package's profiles folder --------

    def package_dir(self, root):
        exe = self.locate(root)
        if not exe:
            return None
        # .../dependencies/linpack/linpack_xeon64.exe
        return os.path.dirname(os.path.dirname(os.path.dirname(exe)))

    def profiles(self, root):
        """(name, path) for each saved config, the package's Default first."""
        package = self.package_dir(root)
        if not package:
            return []
        found = [(os.path.splitext(os.path.basename(p))[0], p) for p in
                 glob.glob(os.path.join(package, "profiles", "*.json"))]
        return sorted(found, key=lambda item: (
            item[0].lower() != self.DEFAULT_PROFILE.lower(), item[0].lower()))

    @classmethod
    def profile_values(cls, path):
        """A config.json read back into this tab's settings."""
        with open(path, "r", encoding="utf-8") as handle:
            data = json.load(handle)
        block = data.get("settings", {}) or {}
        tests = data.get("tests", {}) or {}
        order = data.get("test order") or sorted(tests, key=str)
        chain = []
        for key in order:
            test = tests.get(str(key))
            if test:
                chain.append({name: int(test.get(name, default))
                              for name, default in cls.TEST_DEFAULTS.items()})
        return {
            "tests": chain,
            "residual_check": bool(block.get("stop after residual mismatch",
                                             False)),
            "affinity": str(block.get("KMP_AFFINITY", "") or ""),
            "reduce_below": int(block.get("reduce output below X problem size",
                                          0) or 0),
            "track_below": int(block.get("track stats below X problem size",
                                         0) or 0),
        }

    @staticmethod
    def test_memory_gb(test):
        """What one test's matrix takes, which is most of what it uses."""
        size = int(test.get("problem size", 0) or 0)
        lda = int(test.get("leading dimension", 0) or 0) or size
        return 8.0 * lda * size / (1024 ** 3)

    @classmethod
    def clean_test(cls, test):
        """One test with every value present and the leading dimension sane.

        0 means "work it out", and a leading dimension smaller than the
        problem is one the benchmark cannot use, so both get Intel's figure.
        """
        clean = {name: int(test.get(name, default) or 0)
                 for name, default in cls.TEST_DEFAULTS.items()}
        clean["minutes"] = max(1, clean["minutes"])
        clean["problem size"] = max(1000, clean["problem size"])
        if clean["leading dimension"] < clean["problem size"]:
            clean["leading dimension"] = leading_dimension(
                clean["problem size"])
        return clean

    @classmethod
    def config_json(cls, config):
        """The config.json these settings make, as a dict."""
        chain = [cls.clean_test(t) for t in config.get("tests") or []]
        if not chain:
            chain = [dict(cls.TEST_DEFAULTS)]
        block = {
            "reduce output below X problem size":
                int(config.get("reduce_below", 0) or 0),
            "track stats below X problem size":
                int(config.get("track_below", 0) or 0),
            "stop after residual mismatch":
                bool(config.get("residual_check", False)),
        }
        # linpack.js reads this as `config.settings.KMP_AFFINITY ?? ""`, so
        # leaving the key out is the same as an empty one -- and it is what
        # the configurations people actually pass around look like.
        affinity = str(config.get("affinity", "")).strip()
        if affinity:
            block["KMP_AFFINITY"] = affinity
        return {
            "test order": list(range(1, len(chain) + 1)),
            "settings": block,
            "tests": {str(i): test for i, test in enumerate(chain, 1)},
        }

    @classmethod
    def config_text(cls, config):
        return json.dumps(cls.config_json(config), indent=2) + "\n"

    def profile_preview(self, config):
        return self.config_text(config).rstrip()

    profile_hint = ("Saved as <name>.json in Linpack Extended's profiles "
                    "folder, and shown as a button on Quick Start.")
    profile_preview_label = "config.json"

    @staticmethod
    def clean_profile_name(name):
        return re.sub(r'[<>:"/\\|?*\x00-\x1f]', "", str(name)).strip().strip(".")

    def save_profile(self, root, name, config):
        package = self.package_dir(root)
        name = self.clean_profile_name(name)
        if not package or not name:
            raise ToolUnavailable("A config needs a name and Linpack Extended.")
        path = os.path.join(package, "profiles", name + ".json")
        self._write(path, self.config_text(config))
        return path

    # -- Quick Start ------------------------------------------------------

    def _run_options(self):
        return {"threads": hardware.logical_cores(), "show_window": True}

    def quick_config(self, root):
        """The 11GB config when it is there, otherwise the first one saved."""
        config = self.defaults()
        config["tests"] = [dict(self.TEST_DEFAULTS)]
        profiles = dict(self.profiles(root))
        name = (self.QUICK_PROFILE if self.QUICK_PROFILE in profiles
                else next(iter(profiles), None))
        if name:
            try:
                config.update(self.profile_values(profiles[name]))
            except (OSError, ValueError):
                pass
        config.update(self._run_options())
        return config

    def quick_profile_name(self, root):
        names = [name for name, _ in self.profiles(root)]
        if self.QUICK_PROFILE in names:
            return self.QUICK_PROFILE
        return names[0] if names else ""

    def quick_actions(self, root):
        """A button per saved config, the package's Default first."""
        actions = []
        for name, path in self.profiles(root):
            try:
                values = self.profile_values(path)
            except (OSError, ValueError):
                continue
            actions.append((name, dict(self.defaults(), **values,
                                       **self._run_options())))
        return actions or [("Start", self.quick_config(root))]

    def quick_summary(self, root):
        names = [name for name, _ in self.profiles(root)]
        if not names:
            return "No saved configs -- make one on the Linpack Extended tab."
        return "Run " + " / ".join(names)

    def build(self, config, root):
        """Run the package the way the package runs itself.

        Linpack Extended is a Node driver around the same Intel binary, and it
        is worth using rather than going around: it writes the input file,
        chains tests, keeps Min/Avg/Max GFlops per problem size, and stops on
        a bad solve or a residual that moved -- printing "FAIL - severe
        instability detected" or "RESIDUAL MISMATCH - instability detected",
        which are already the patterns in errors.LINPACK.

        Its settings live in config.json at the root of the package, and the
        path is hard-coded in linpack.js, so the chosen config is written
        there. The one that shipped is copied aside the first time rather
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

        settings_path = os.path.join(package, "config.json")
        original = settings_path + ".roch-original"
        try:
            if os.path.exists(settings_path) and not os.path.exists(original):
                shutil.copyfile(settings_path, original)
        except OSError:
            pass

        document = self.config_json(config)
        self._write(settings_path, json.dumps(document, indent=2) + chr(10))
        chain = list(document["tests"].values())

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

        summary = ("Linpack Extended "
                   + ", ".join("n={} ({:.1f} GB, {} min)".format(
                       t["problem size"], self.test_memory_gb(t), t["minutes"])
                       for t in chain)
                   + ", " + str(threads) + " threads")
        # linpack.js ends by itself once the last test's minutes are up and
        # says so; this is the countdown, with a little room for it to.
        seconds = sum(t["minutes"] for t in chain) * 60 + 60
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
