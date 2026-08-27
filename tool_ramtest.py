"""RAM Test Pro, launched with a chosen configuration.

RAM Test Pro has no command line at all. What it has is a file --
``config/current_config.txt`` -- holding the name of the .cfg it should load
on startup, which is enough to pick a profile from here. Everything else
(thread count, memory to reserve, error limit) is set in its own window.

So this adapter does the one thing it can do honestly: write the profile
choice, start the program in its own folder, and say plainly in the UI that
the rest is on the tool's own screen.
"""

import glob
import os

from toolbase import Field, LaunchSpec, Preset, Tool, ToolUnavailable


class RamTestPro(Tool):
    key = "ramtest"
    name = "RAM Test Pro"
    blurb = (
        "A second opinion on memory, with a different test set from TM5. "
        "Worth running when TM5 passes but the machine still misbehaves."
    )
    exe_globs = (
        "ram_test_pro*/*/RAM Test Pro.exe",
        "ram_test_pro*/RAM Test Pro.exe",
        "RAMTestPro*/RAM Test Pro.exe",
        "RAM Test Pro.exe",
    )
    console = False
    detection_note = (
        "RAM Test Pro reports errors in its own window, and its thread and "
        "memory settings live there too. This program selects the profile "
        "and starts it; watch the window for the error count."
    )

    # The preset picker on the panel already offers exactly these, so
    # the panel hides this field rather than showing the same list twice.
    preset_field = "config"

    fields = (
        Field("config", "Configuration", "choice", "",
              hint="The .cfg files in RAM Test Pro's config folder."),
        Field("duration", "Stop after", "int", 0, minimum=0, maximum=100000,
              unit="min", hint="0 runs until you press Stop."),
    )

    quick_start = {
        "preset": "DDR4_DDR5_universal",
        "values": {"duration": 0},
        "note": "The modern-platform profile, no time limit.",
    }

    _NOTES = {
        "DDR4_DDR5_universal": "The one to use on any modern platform.",
        "DDR3_DDR2_universal": "For older machines.",
        "default": "The stock profile.",
    }

    def configs(self, root):
        """The .cfg files in the config folder, as (name, path) pairs."""
        exe = self.locate(root)
        if not exe:
            return []
        folder = os.path.join(os.path.dirname(exe), "config")
        found = []
        for path in sorted(glob.glob(os.path.join(folder, "*.cfg"))):
            found.append((os.path.splitext(os.path.basename(path))[0], path))
        # The DDR4/DDR5 profile first: it is the right answer on anything
        # this program is likely to be run on.
        found.sort(key=lambda pair: (not pair[0].startswith("DDR4"), pair[0]))
        return found

    def note_for(self, config_name):
        return self._NOTES.get(config_name, "")

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "'RAM Test Pro.exe' was not found. Expected a folder like "
                "ram_test_pro_1.5.0 beside this program."
            )

        folder = os.path.dirname(exe)
        available = dict(self.configs(root))
        chosen = str(config.get("config", "")).strip()
        if chosen not in available and available:
            chosen = next(iter(available))

        if chosen:
            # The file holds a bare filename, not a path -- that is the
            # format already in the box, and it is read relative to config/.
            self._write(
                os.path.join(folder, "config", "current_config.txt"),
                chosen + ".cfg",
            )

        return LaunchSpec(
            argv=[exe],
            cwd=folder,
            console=False,
            watch_files=[
                os.path.join(folder, name)
                for name in ("log.txt", "ramtest.log", "errors.txt")
            ],
            error_key=self.key,
            summary="RAM Test Pro " + (chosen or "default"),
            duration_seconds=int(config.get("duration", 0)) * 60,
        )

    def presets_for(self, root):
        made = [
            Preset(name, {"config": name}, self.note_for(name))
            for name, _ in self.configs(root)
        ]
        return tuple(made) or (Preset("None found", {}, ""),)
