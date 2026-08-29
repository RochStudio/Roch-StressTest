"""3DMark 11, opened as a graphics load.

Its window is where everything about a 3DMark run is decided: which
benchmark, how many loops, which adapter. There is a command-line runner
beside it, ``bin\x64\3DMark11Cmd.exe``, which can set all of that from
outside -- but it needs the Professional edition, and Advanced (which the
free legacy key gives) unlocks looping in the window instead. Offering those
settings here would mean offering most installations a set of controls that
answer "you do not have a licence for this", so this opens the window and
lets it be driven there.

Looping is the part that matters: run once, 3DMark renders its scenes,
reports a score and exits, which is a benchmark rather than a load. Looped,
it holds the GPU at a game-like duty cycle until it is stopped -- a different
kind of stress from memtest_vulkan, shaders and the whole board rather than
the memory alone.

Worth being clear about: a looping benchmark reports nothing when the machine
is merely wrong. It catches a driver reset, a hang, or a crash -- which is
how an unstable GPU usually fails -- but it does not verify a single pixel.
memtest_vulkan is the one that checks its answers.
"""
import os

from core.toolbase import Field, LaunchSpec, Tool, ToolUnavailable

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
        "reset or a hang rather than a wrong answer. Opens in its own window, "
        "where Advanced can set a loop count; the command-line runner that "
        "does it unattended needs the Professional edition."
    )
    # The application itself, not 3DMarkLauncher.exe beside it. The launcher
    # is a stub: it starts this and exits immediately, so a run supervised
    # through it reported "finished with no errors" a second after starting
    # while the benchmark it had just opened carried on unwatched.
    exe_globs = ("3DMark 11*/bin/x64/3DMark11.exe",)
    external_globs = tuple(
        os.path.join(root, "bin", "x64", "3DMark11.exe")
        for root in SEARCH_ROOTS
    )
    console = True
    detection_note = (
        "A looping benchmark has no right answer to check, so what stops the "
        "run is a failed workload, a removed or hung device, or the process "
        "dying -- the shapes an unstable graphics card actually fails in. "
        "Run from its own window there is no output to read, so the time "
        "limit and a crash are what this program can see."
    )

    fields = (
        Field("duration", "Stop after", "int", 0, minimum=0, maximum=100000,
              unit="min",
              hint="0 lets the loop you set in 3DMark's own window run until "
                   "you press Stop."),
    )

    # Configured in 3DMark's own window, so there is no tab here. Its
    # presets, loop count and adapter all live in that window, and the
    # command-line runner that could set them from outside needs the
    # Professional edition -- so a tab here would be a tab of settings most
    # installations cannot use.
    has_tab = False

    presets = ()

    quick_start = {
        "values": {"duration": 0},
        "note": "Opens 3DMark 11, so the preset and the loop are the ones you "
                "set in its window. Stop ends it.",
    }

    def quick_actions(self, root):
        """One button, and it opens the tool rather than starting a run."""
        return [("Open", self.quick_config(root))]

    def quick_summary(self, root):
        """The card has no preset to name, so it says what will happen."""
        limit = int(self.quick_config(root).get("duration", 0) or 0)
        # Nothing worth a line of its own when there is no limit: the
        # button says "Open" and the note underneath says the rest.
        return str(limit) + " min" if limit else ""

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "3DMark 11 was not found. Expected bin\\x64\\3DMark11.exe "
                "under Program Files\\Futuremark\\3DMark 11."
            )

        # The window, and only the window. Every edition can open it, and
        # Advanced -- which the free legacy key gives -- can set a loop count
        # in it. The command-line runner could drive it from outside, but it
        # needs the Professional edition, so most installations would be
        # offered a control that answers "you do not have a licence for
        # this". The runner still holds this to a time limit and still
        # notices it dying, which is most of what supervision is for here.
        return LaunchSpec(
            argv=[exe],
            cwd=os.path.dirname(exe),
            console=False,
            error_key=self.key,
            summary="3DMark 11 (preset and loop set in its own window)",
            duration_seconds=int(config.get("duration", 0)) * 60,
            leave_open=True,
        )
