"""memtest_vulkan: the GPU's memory, tested the way TM5 tests the DIMMs.

https://github.com/GpuZelenograd/memtest_vulkan

The only tool here that tests video memory, and the reason it belongs beside
the others: an unstable memory overclock on a graphics card shows up as
artefacts, driver resets and crashes in games, and none of the CPU-side tools
in this program would ever see it.

It is a console program that writes both to the console and to
``memtest_vulkan.log`` beside the executable, so it can be shown in its own
window and still be watched. Its wording is taken from the binary rather than
guessed:

    "  981 iteration. Passed 30.0198 seconds  written: ... checked: ..."
    "Error found. Mode , total errors 0x out of 0x (%)"
    "Errors address range:"

It runs until it is stopped -- the "standard 5-minute test" it announces is
the length of the first pass, not the run -- so the time limit is the
runner's, as with Prime95.

One quirk worth knowing: it re-executes itself, so the process that is
started is a console that spawns a worker. Killing it needs the whole tree,
which is what the runner does anyway.
"""

import os

from core import errors
from core.toolbase import Field, LaunchSpec, Preset, Tool, ToolUnavailable


class MemtestVulkan(Tool):
    key = "memtest_vulkan"
    name = "memtest Vulkan"
    blurb = (
        "Video memory, tested through Vulkan. Nothing else here looks at the "
        "graphics card at all, and a bad memory overclock on it shows up as "
        "artefacts and driver resets rather than as anything the CPU tests "
        "would notice."
    )
    exe_globs = (
        "memtest_vulkan*/memtest_vulkan*.exe",
        "memtest_vulkan*.exe",
    )
    console = True
    detection_note = (
        "Output is read live. 'Error found' and 'Errors address range:' are "
        "its own failure wording, taken from the binary; every good pass "
        "prints 'iteration. Passed'. A run that ends without having been "
        "asked to stop is reported as stopped rather than passed."
    )

    fields = (
        Field("device", "GPU index", "int", 1, minimum=1, maximum=16,
              hint="Which device to test, as memtest_vulkan numbers them in "
                   "its own listing. 1 is the first."),
        Field("memory", "Memory to test", "int", 0, minimum=0, maximum=1048576,
              unit="MB",
              hint="0 lets it size the test from the card's free memory, "
                   "which is what you usually want."),
        Field("duration", "Stop after", "int", 30, minimum=0, maximum=100000,
              unit="min",
              hint="memtest_vulkan runs until it is stopped, so this limit "
                   "is enforced here. 0 runs until you press Stop."),
        Field("show_window", "Show its window", "bool", True,
              hint="Runs it in its own console. Failures are then read from "
                   "memtest_vulkan.log, which carries the same text."),
    )

    quick_start = {
        "preset": "Standard",
        "values": {"duration": 15},
    }

    presets = (
        Preset("Standard", {"device": 1, "memory": 0},
               "The first GPU, with the test sized from its free memory."),
        Preset("Second GPU", {"device": 2, "memory": 0},
               "For a machine with a second card, or an integrated one."),
        Preset("Custom", {}, "Whatever is in the boxes below."),
    )

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "memtest_vulkan was not found. Put memtest_vulkan-vX.Y.Z.exe "
                "in a memtest_vulkan folder beside this program -- it is a "
                "single file, from github.com/GpuZelenograd/memtest_vulkan."
            )

        folder = os.path.dirname(exe)
        # It writes this beside itself, and appends across runs. Watched
        # rather than deleted, so the card's history is kept; the runner
        # reads only what is added after the run starts.
        log = os.path.join(folder, "memtest_vulkan.log")

        # It takes the device index and the memory size as bare positional
        # arguments, in that order. With neither it prompts, which is fine
        # for a person and useless for a launcher.
        argv = [exe, str(int(config.get("device", 1)))]
        memory = int(config.get("memory", 0))
        if memory > 0:
            argv.append(str(memory))

        show = bool(config.get("show_window", True))
        return LaunchSpec(
            argv=argv,
            cwd=folder,
            console=not show,
            watch_files=[log] if show else [],
            error_key=self.key,
            summary=(
                "memtest_vulkan GPU " + str(int(config.get("device", 1)))
                + ", " + (str(memory) + " MB" if memory else "auto size")
            ),
            duration_seconds=int(config.get("duration", 0)) * 60,
            # Written years ago and never wired in. Without them, "no device
            # selected" or an early exit during init leaves a non-zero exit
            # code and nothing to explain it, and the runner reports the
            # graphics card as faulty when memtest_vulkan simply never
            # started.
            abort_patterns=errors.MEMTEST_VULKAN_ABORTED,
            creation_flags=(self._new_console_flags() if show
                            else self._no_window_flags()),
        )
