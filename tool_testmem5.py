"""TestMem5, launched with one of the configurations in its bin folder.

TM5 takes a single argument: the path to the .cfg that describes the test
sequence. Everything that makes one TM5 profile different from another --
which tests run, in what order, for how many cycles, how large the testing
window is -- lives in that file, which is why this adapter has almost no
settings of its own. Picking "Absolut @ anta777" *is* the configuration.

The one thing worth knowing: TM5 is a windowed program with no log. Its error
count is drawn in its own window and written nowhere else, so a failure at 3am
is a number on a screen nobody is looking at. This adapter still watches for a
log in case a profile is set to write one, and the runner catches the process
dying, but the honest answer is in ``detection_note`` and the UI shows it.
"""

import glob
import os
import shutil

from toolbase import Field, LaunchSpec, Preset, Tool, ToolUnavailable


class TestMem5(Tool):
    key = "testmem5"
    name = "TestMem5"
    blurb = (
        "The memory test overclockers actually use. The anta777 profiles are "
        "the ones worth running: Extreme for a first pass, Absolut when you "
        "want to be sure."
    )
    exe_globs = ("TestMem5*/TM5.exe", "TM5*/TM5.exe", "TM5.exe")
    console = False
    detection_note = (
        "TM5 shows its error count in its own window and writes no log, so "
        "leave that window visible. This program catches a crash or an early "
        "exit, and reads any log a profile is configured to write, but it "
        "cannot read the on-screen counter."
    )

    # The preset picker on the panel already offers exactly these, so
    # the panel hides this field rather than showing the same list twice.
    preset_field = "config"

    fields = (
        Field("config", "Configuration", "choice", "",
              hint="The .cfg files in TestMem5's bin folder."),
        Field("duration", "Stop after", "int", 0, minimum=0, maximum=100000,
              unit="min",
              hint="0 lets the profile run its own cycle count to the end."),
    )

    # Ordered roughly by how hard they lean on the memory subsystem, so the
    # list reads top to bottom as "quick check" through "leave it overnight".
    _PREFERRED_ORDER = (
        "Super Light 2 @ anta777",
        "Universal 2 @ LMhz",
        "Default @ serj",
        "1usmus v3 @ 1usmus",
        "Heavy @ anta777",
        "Extreme @ anta777",
        "Absolut @ anta777",
        "DDR5 Intel @ anta777",
        "DDR5 Ryzen3D @ anta777",
    )

    _NOTES = {
        "Super Light 2 @ anta777":
            "Minutes, not hours. A smoke test for a setting you just changed.",
        "Universal 2 @ LMhz":
            "A broad general-purpose sweep.",
        "Default @ serj":
            "TM5's own stock profile. Mild by modern standards.",
        "1usmus v3 @ 1usmus":
            "The older community standard, still good at finding tRFC and "
            "tREFI problems.",
        "Heavy @ anta777":
            "A serious pass in about an hour.",
        "Extreme @ anta777":
            "The usual verdict profile. Three cycles, roughly 2-3 hours.",
        "Absolut @ anta777":
            "The strictest of the set. Slow, and it finds what Extreme "
            "misses.",
        "DDR5 Intel @ anta777":
            "Tuned for DDR5 on Intel platforms.",
        "DDR5 Ryzen3D @ anta777":
            "Tuned for DDR5 on Ryzen X3D parts.",
    }

    def configs(self, root):
        """Every .cfg in TM5's bin folder, best-known ones first.

        Returned as ``(display name, full path)``. The display name drops the
        extension, because "Absolut @ anta777" is what everyone calls it and
        "Absolut @ anta777.cfg" is just noisier.
        """
        exe = self.locate(root)
        if not exe:
            return []
        found = {}
        for path in glob.glob(os.path.join(os.path.dirname(exe), "bin", "*.cfg")):
            name = os.path.splitext(os.path.basename(path))[0]
            # MT.cfg is the copy this program writes for TM5 to fall back on.
            # Listing it would offer "whatever you picked last time" as if it
            # were a profile of its own.
            if name.upper() == "MT":
                continue
            found[name] = path
        ordered = [(n, found.pop(n)) for n in self._PREFERRED_ORDER if n in found]
        ordered.extend(sorted(found.items()))
        return ordered

    def note_for(self, config_name):
        return self._NOTES.get(config_name, "")

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "TM5.exe was not found. Expected a TestMem5 folder beside "
                "this program."
            )

        available = dict(self.configs(root))
        if not available:
            raise ToolUnavailable(
                "TestMem5 has no .cfg files in its bin folder, so there is "
                "nothing to run."
            )

        chosen = str(config.get("config", "")).strip()
        if chosen not in available:
            chosen = next(iter(available))
        cfg_path = available[chosen]

        folder = os.path.dirname(exe)

        # TM5 takes the .cfg as its argument, but falls back to bin\MT.cfg
        # when it does not like the one it was given. Writing the same file
        # to both places means the profile shown in this window is the
        # profile that runs, whichever path TM5 takes.
        try:
            shutil.copyfile(cfg_path, os.path.join(folder, "bin", "MT.cfg"))
        except OSError:
            pass

        # A profile that logs writes beside the executable; harmless to watch
        # when none appears.
        watch = [
            os.path.join(folder, name)
            for name in ("MT.log", "TM5.log", "MemTest.log")
        ]

        return LaunchSpec(
            argv=[exe, cfg_path],
            cwd=folder,
            console=False,
            watch_files=watch,
            error_key=self.key,
            summary="TestMem5 " + chosen,
            duration_seconds=int(config.get("duration", 0)) * 60,
        )

    def presets_for(self, root):
        """Presets built from whatever .cfg files are actually present."""
        made = []
        for name, _ in self.configs(root):
            made.append(Preset(name, {"config": name}, self.note_for(name)))
        return tuple(made) or (Preset("None found", {}, ""),)
