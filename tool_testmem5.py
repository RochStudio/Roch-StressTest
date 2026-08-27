"""TestMem5, launched with one of the configurations in its bin folder.

Everything that makes one TM5 profile different from another -- which tests
run, in what order, for how many cycles, how large the testing window is --
lives in a .cfg, which is why this adapter has almost no settings of its own.
Picking "Absolut @ anta777" *is* the configuration.

Getting TM5 to load one is the part worth writing down. Every guide for the
older TM5 says to pass the .cfg as the first argument, and 0.13.1 answers that
with "Something is wrong on the command line" in a message box and then sits
there testing nothing. Its real usage string exists only inside TM5.dll:

    TM5.exe Config File="CoolCmd @ CoolCmd.cfg" Minutes=180

A bare file name, resolved inside bin, with a parameter name that has a space
in it -- and Minutes, which lets TM5 hold itself to a time limit instead of
being killed at one.

TM5 is windowed, and it is widely held that its error count exists only on
that window. Not in 0.13.1: it appends to ``Log.txt`` beside TM5.exe, with the
configuration it loaded, the memory it took, every error, and how the run
ended. That file is watched, so a failure at 3am is caught rather than being a
number on a screen nobody is looking at.

The error strings in ``errors.TESTMEM5`` are TM5's own message templates,
taken from the strings in TM5.exe and TM5.dll rather than guessed -- including
the ones that must *not* count, because TM5 separates a failure of the memory
under test from a failure of TM5 itself and says so in as many words.
"""

import glob
import os
import re

import errors

from toolbase import Field, LaunchSpec, Preset, Tool, ToolUnavailable

# The one file this adapter writes into TM5's bin folder, used only when
# a cycle count overrides the profile's own.
WORKING_NAME = "Roch active"
WORKING_CFG = WORKING_NAME + ".cfg"


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
        "TM5 0.13.1 appends to Log.txt beside TM5.exe, and that is read live: "
        "'Error in test #N', a non-zero error total, and a crash blamed on "
        "the tested memory all stop the run. Its own failures -- memory it "
        "could not allocate or lock -- are deliberately not reported as "
        "instability, because TM5 says outright that they are not."
    )

    # The preset picker on the panel already offers exactly these, so
    # the panel hides this field rather than showing the same list twice.
    preset_field = "config"

    fields = (
        Field("config", "Configuration", "choice", "",
              hint="The .cfg files in TestMem5's bin folder."),
        Field("cycles", "Cycles", "int", 0, minimum=0, maximum=9999,
              hint="0 keeps the profile's own cycle count. Anything else "
                   "overrides it, and the run ends when the cycles do."),
        Field("duration", "Stop after", "int", 0, minimum=0, maximum=100000,
              unit="min",
              hint="0 lets the profile run its own cycle count to the end."),
    )

    # What the Quick Start page runs, and what this tab opens on.
    quick_start = {
        "preset": "1usmus v3 @ 1usmus",
        "values": {"cycles": 25, "duration": 0},
        "note": "25 cycles, no time limit -- it ends when the cycles do.",
    }

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
            # Working files rather than profiles: the copy this program
            # writes when a cycle count is overridden, and the name TM5
            # itself falls back to. Listing either would offer "whatever
            # you ran last time" as though it were a profile.
            if name.upper() in ("MT", WORKING_NAME.upper()):
                continue
            found[name] = path
        ordered = [(n, found.pop(n)) for n in self._PREFERRED_ORDER if n in found]
        ordered.extend(sorted(found.items()))
        return ordered

    def note_for(self, config_name):
        return self._NOTES.get(config_name, "")

    @staticmethod
    def _write_with_cycles(source, destination, cycles):
        """Copy a .cfg, changing only the Cycles line.

        Done on bytes rather than as text on purpose. These files are TM5's
        own format, written by several different authors, and the only thing
        being changed is one number: reading and rewriting them as text would
        risk re-encoding a byte or normalising a line ending somewhere else in
        a file that nothing here understands in full.

        Every profile in the folder has exactly one ``Cycles=`` line, in
        [Main Section] at the top, which is why the first match is the right
        one. A file without the key is copied through unchanged -- better a
        run at the profile's own cycle count than no run at all.
        """
        with open(source, "rb") as handle:
            data = handle.read()
        patched, count = re.subn(
            rb"(?m)^Cycles=\d+", b"Cycles=" + str(cycles).encode("ascii"),
            data, count=1,
        )
        with open(destination, "wb") as handle:
            handle.write(patched if count else data)

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
        bin_folder = os.path.join(folder, "bin")

        # TM5 0.13.1 does not take a path. Its usage string, which is in
        # TM5.dll and nowhere else, is:
        #
        #     TM5.exe Config File="CoolCmd @ CoolCmd.cfg" Minutes=180
        #
        # The value is a bare file name resolved inside the bin folder. A
        # path -- a "bin\" prefix, or even a correct absolute one -- gets
        # "Something is wrong on the command line" in a message box, and the
        # window then
        # sits there testing nothing. Passing the .cfg as a plain first
        # argument, which is what every guide for the older TM5 says to do,
        # gets the same box.
        #
        # The parameter name has a space in it, so this is built as a command
        # line rather than a list: no argument list survives being re-quoted
        # into `Config File="..."`.
        file_name = os.path.basename(cfg_path)

        cycles = int(config.get("cycles", 0))
        if cycles > 0:
            # The cycle count lives inside the .cfg, so overriding it means
            # writing a copy. One fixed working file, never one per profile,
            # so the bin folder does not fill up with near-duplicates -- and
            # the stock profiles are never edited, because a cycle count set
            # here is a property of this run, not of the profile.
            try:
                self._write_with_cycles(
                    cfg_path, os.path.join(bin_folder, WORKING_CFG), cycles
                )
                file_name = WORKING_CFG
            except OSError:
                cycles = 0

        parts = ['"' + exe + '"', 'Config File="' + file_name + '"']

        # TM5's own Minutes= limit is deliberately not used. It works, but
        # loosely -- Minutes=2 on this machine stopped testing at 3:35 -- and
        # a tool that stops slightly early reports a pass for a test that did
        # not run as long as it was asked to. The runner's wall clock is
        # exact, so it is the only clock in play.
        minutes = int(config.get("duration", 0))

        # TM5 0.13.1 appends to Log.txt beside the executable -- the
        # configuration it loaded, how much memory it took, every error, and
        # how the run ended. Crash.log is the other half: TM5 points at it by
        # name when it dies, and says the likely cause is the memory under
        # test.
        #
        # Neither is deleted first. The watcher records the size it starts at
        # and reads only what is appended, so a run is never contaminated by
        # the last one and the user keeps their TM5 history.
        watch = [
            os.path.join(folder, name)
            for name in ("Log.txt", "Crash.log")
        ]

        summary = "TestMem5 " + chosen
        if cycles > 0:
            summary += ", " + str(cycles) + " cycles"
        if minutes > 0:
            summary += ", " + str(minutes) + " min"

        return LaunchSpec(
            argv=[exe, "Config", 'File="' + file_name + '"'],
            cmdline=" ".join(parts),
            cwd=folder,
            console=False,
            watch_files=watch,
            error_key=self.key,
            summary=summary,
            duration_seconds=minutes * 60,
            # TM5 does not exit when it is done. It writes "Testing
            # completed", leaves the window up, and waits. Without these a run
            # of a fixed number of cycles -- the right way to use TM5, and the
            # one case with no time limit to fall back on -- would never be
            # reported at all.
            completion_patterns=errors.TESTMEM5_COMPLETE,
            abort_patterns=errors.TESTMEM5_ABORTED,
            # TM5 leaves its window up when it finishes, showing the cycle
            # count and the error total. Killing it the moment the log said
            # "Testing completed" threw away the one summary TM5 gives you.
            leave_open=True,
        )

    def presets_for(self, root):
        """Presets built from whatever .cfg files are actually present."""
        made = []
        for name, _ in self.configs(root):
            made.append(Preset(name, {"config": name}, self.note_for(name)))
        return tuple(made) or (Preset("None found", {}, ""),)
