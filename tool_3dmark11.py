"""3DMark 11, looped, as a graphics load.

The GUI launcher takes no useful arguments, but ``bin\\x64\\3DMark11Cmd.exe``
does, and its embedded usage text is where these came from:

    --definition=<benchmark.xml>   Name of benchmark definition XML file.
    --loop[=<count>]               The number of times to loop benchmark.
                                   Count 0 or omitting count means infinite.
    --audio[=on|=off]              Play audio (default on).
    --systeminfo[=on|=off]         Collect SystemInfo (default off).
    --adapter=<index>              Index of used DXGI adapter.

``--loop=0`` is what makes this usable here: without it 3DMark runs its
scenes once, reports a score and exits, which is a benchmark rather than a
load. Looped, it holds the GPU at a game-like duty cycle until the runner
stops it, which is a different kind of stress from memtest_vulkan -- shaders
and the whole board rather than the memory alone.

The definition files sit beside the executable (performance, extreme, entry),
so the presets are read from what is actually installed rather than assumed.

Worth being clear about: a looping benchmark reports nothing when the machine
is merely wrong. It catches a driver reset, a hang, or a crash -- which is
how an unstable GPU usually fails -- but it does not verify a single pixel.
memtest_vulkan is the one that checks its answers.
"""

import glob
import os

import errors
from toolbase import Field, LaunchSpec, Preset, Tool, ToolUnavailable

SEARCH_ROOTS = (
    r"C:\Program Files\Futuremark\3DMark 11",
    r"C:\Program Files (x86)\Futuremark\3DMark 11",
    r"C:\Program Files\Futuremark\3DMark11",
)

# The definition file names, and what each one is for.
DEFINITIONS = {
    "performance_definition.xml": "Performance -- 1280x720, the usual preset.",
    "extreme_definition.xml": "Extreme -- 1920x1080, a much heavier load.",
    "entry_definition.xml": "Entry -- 1024x600, for weak cards.",
    "custom_performance_definition.xml": "Custom performance definition.",
    "custom_extreme_definition.xml": "Custom extreme definition.",
    "custom_entry_definition.xml": "Custom entry definition.",
}


class ThreeDMark11(Tool):
    key = "3dmark11"
    name = "3DMark 11"
    blurb = (
        "A looping graphics load: shaders, geometry and the whole board at a "
        "game-like duty cycle, which is how an unstable GPU core clock "
        "usually gets caught. It checks nothing, so what it finds is a driver "
        "reset or a hang rather than a wrong answer."
    )
    exe_globs = ("3DMark 11*/bin/x64/3DMark11Cmd.exe",)
    external_globs = tuple(
        os.path.join(root, "bin", "x64", "3DMark11Cmd.exe")
        for root in SEARCH_ROOTS
    )
    console = True
    detection_note = (
        "A looping benchmark has no right answer to check, so what stops the "
        "run is 3DMark reporting a failed workload, a removed or hung device, "
        "or the process dying -- the shapes an unstable graphics card "
        "actually fails in."
    )

    fields = (
        Field("definition", "Preset", "choice", "", choices=[],
              hint="The benchmark definition to loop."),
        Field("loops", "Loops", "int", 0, minimum=0, maximum=10000,
              hint="0 loops forever, which is what a soak wants. Any other "
                   "number stops after that many runs."),
        Field("adapter", "GPU index", "int", 0, minimum=0, maximum=16,
              hint="DXGI adapter index. 0 is the primary card."),
        Field("audio", "Play audio", "bool", False,
              hint="3DMark plays sound by default, which is not welcome from "
                   "something left running for an hour."),
        Field("duration", "Stop after", "int", 30, minimum=0, maximum=100000,
              unit="min",
              hint="Enforced here, since a looping benchmark never ends on "
                   "its own. 0 runs until you press Stop."),
    )

    quick_start = {
        "preset": "Performance",
        "values": {"loops": 0, "duration": 30, "audio": False},
        "note": "Performance preset on a loop, 30 minutes, sound off.",
    }

    _LABELS = {
        "performance_definition.xml": "Performance",
        "extreme_definition.xml": "Extreme",
        "entry_definition.xml": "Entry",
    }

    def definitions(self, root):
        """The definition files present, as (label, filename) pairs."""
        exe = self.locate(root)
        if not exe:
            return []
        folder = os.path.dirname(exe)
        found = []
        for path in sorted(glob.glob(os.path.join(folder, "*_definition.xml"))):
            name = os.path.basename(path)
            found.append((self._LABELS.get(name, name), name))
        # Performance first: it is the preset almost everyone means.
        found.sort(key=lambda pair: (pair[0] != "Performance", pair[0]))
        return found

    def note_for(self, label):
        for name, description in DEFINITIONS.items():
            if self._LABELS.get(name, name) == label:
                return description
        return ""

    def presets_for(self, root):
        made = [
            Preset(label, {"definition": name}, self.note_for(label))
            for label, name in self.definitions(root)
        ]
        return tuple(made) or (Preset("None found", {}, ""),)

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "3DMark 11 was not found. Expected its bin\\x64 folder under "
                "Program Files\\Futuremark\\3DMark 11."
            )

        available = dict((name, label)
                         for label, name in self.definitions(root))
        wanted = str(config.get("definition", "")).strip()
        if wanted not in available and available:
            wanted = next(iter(available))
        if not wanted:
            raise ToolUnavailable(
                "3DMark 11 has no benchmark definition files beside its "
                "executable, so there is nothing to run."
            )

        loops = int(config.get("loops", 0))
        argv = [
            exe,
            "--definition=" + wanted,
            "--loop=" + str(loops),
            "--audio=" + ("on" if config.get("audio") else "off"),
            "--systeminfo=off",
            "--adapter=" + str(int(config.get("adapter", 0))),
        ]

        return LaunchSpec(
            argv=argv,
            cwd=os.path.dirname(exe),
            console=True,
            error_key=self.key,
            summary=(
                "3DMark 11 " + available.get(wanted, wanted) + ", "
                + ("looping" if loops == 0 else str(loops) + " runs")
            ),
            duration_seconds=int(config.get("duration", 0)) * 60,
            creation_flags=self._no_window_flags(),
            # Looping is a Professional-edition feature. A Basic install
            # says so and stops, which is a licence problem rather than
            # anything the graphics card did.
            setup_patterns=errors.THREEDMARK_SETUP,
        )
