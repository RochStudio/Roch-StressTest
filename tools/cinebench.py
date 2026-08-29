"""Cinebench, in the six versions worth keeping around.

One tool with the version as its preset rather than six tabs, because that is
what the versions are: the same renderer, re-tuned, and picking R23 instead of
R20 is choosing a preset in every sense that matters here.

Two generations, two command lines, both confirmed by running them:

  R20 and later take named settings --

      Cinebench.exe g_CinebenchCpuXTest=true g_CinebenchMinimumTestDuration=1800

    and print "CB 24164.72 (0.00)" at the end. The duration is in seconds and
    is the thing that makes Cinebench a stress test at all: given one, it
    re-runs the render until that much time has passed, so a 30-minute
    setting is 30 minutes of sustained all-core load. Verified -- 45 seconds
    produced two passes where the default produced one.

  R15 is older and takes switches -- ``-cb_cpux`` for the all-core render.
    It has no duration setting at all, so it can only ever do the single
    run, however long that takes. The panel says so rather than offering a
    loop that would not happen.

A word on what this is for. Cinebench is a benchmark: it reports a score, not
a verdict, and it has no idea whether the answer it computed was right. It
earns its place because a sustained all-core render is the load that finds an
unstable all-core clock or a cooling limit, and because the score falling
between runs is itself a signal -- but a pass here means "it finished", not
"the arithmetic was correct". Prime95 and y-cruncher are the tools that check
their own answers.
"""

import glob
import os

from core.toolbase import Field, LaunchSpec, Tool, ToolUnavailable

# Where each version's executable is, in the order they should be searched.
# BenchMate keeps a version-numbered folder per release, which is why these
# end in a wildcard rather than a fixed number.
VERSIONS = (
    # Oldest first, so the buttons read in the order the versions came out.
    # R11.5 is the same Cinema 4D-based build as R15 and ships a 32-bit
    # executable beside the 64-bit one; the 64-bit is the one to run.
    ("R11.5", ("CINEBENCH R11.5",), "CINEBENCH Windows 64 Bit.exe", "old"),
    ("R15", ("CINEBENCH R15",), "CINEBENCH Windows 64 Bit.exe", "old"),
    ("R15 Extreme", ("CINEBENCH R15 EXTREME",),
     "CINEBENCH Windows 64 Bit.exe", "old"),
    ("R20", ("CINEBENCH R20",), "Cinebench.exe", "new"),
    ("R23", ("CINEBENCH R23",), "Cinebench.exe", "new"),
    ("R24", ("CINEBENCH 2024",), "Cinebench.exe", "new"),
    ("R26", ("CINEBENCH 2026",), "Cinebench.exe", "new"),
)

# Where an installed Cinebench turns up. A copy sitting beside this program
# is preferred over any of these, and is the way to pin an exact build.
#
# BenchMate's app folder used to be searched here and is not any more. The
# copies it keeps are meant to be launched by BenchMate with its own
# environment; run straight out of that folder they cannot resolve their own
# string resources, and Cinebench 2024 comes up titled "StrNotFound" with a
# licence agreement whose Accept and Decline buttons are both labelled
# "StrNotFound" as well. Finding nothing and saying so beats opening that.
SEARCH_ROOTS = (
    r"C:\Program Files\Maxon",
    r"C:\Program Files (x86)\Maxon",
)



class Cinebench(Tool):
    key = "cinebench"
    name = "Cinebench"
    blurb = (
        "A sustained all-core render: the load that finds an unstable all-core "
        "clock or a cooling limit. It reports a score rather than a verdict -- "
        "it does not check its own arithmetic the way Prime95 does -- so a "
        "pass here means it finished, and a score that drops between runs is "
        "worth as much as the number itself."
    )
    console = True
    detection_note = (
        "Cinebench has no notion of a wrong answer, so what is watched for is "
        "it failing to run: a driver or renderer that falls over mid-render. "
        "The score is read off the 'CB' line and reported when the run ends."
    )

    fields = (
        Field("duration", "Stop after", "int", 0, minimum=0, maximum=100000,
              unit="min",
              hint="0 lets it stay open for as many runs as you want."),
    )

    # Configured in Cinebench's own window, so there is no tab here. Every
    # version installed gets a button that opens it, and the run is started
    # there -- which is also the only way to get at the settings a given
    # version actually has, since they differ between R15 and R26.
    has_tab = False

    presets = ()

    quick_start = {
        "values": {"duration": 0},
        "note": "One button per version installed. Each opens that Cinebench; "
                "start the run in its window. The score it prints is kept in "
                "the Log tab.",
    }

    @staticmethod
    def _find(folders, exe_name):
        for root in SEARCH_ROOTS:
            for folder in folders:
                pattern = os.path.join(root, folder, "*", exe_name)
                matches = [m for m in glob.glob(pattern) if os.path.isfile(m)]
                if matches:
                    return sorted(matches)[-1]
                pattern = os.path.join(root, folder, exe_name)
                matches = [m for m in glob.glob(pattern) if os.path.isfile(m)]
                if matches:
                    return sorted(matches)[-1]
        return None

    def installed(self, root):
        """Every Cinebench version present, as (label, path, generation)."""
        found = []
        for label, folders, exe_name, generation in VERSIONS:
            # Beside this program first, so a copy dropped in the folder wins
            # over an installed one.
            # Both the label and the real folder name are tried. Maxon ships
            # 2024 and 2026 in folders called "Cinebench 2024" and "Cinebench
            # 2026" -- neither contains "R24" or "R26", so globbing on the
            # label alone found the installed copies and never a standalone
            # one dropped in beside this program, which is the copy that is
            # supposed to win.
            candidates = []
            for pattern in ["Cinebench*" + label] + list(folders):
                candidates += glob.glob(
                    os.path.join(root, pattern, "**", exe_name),
                    recursive=True)
            local = sorted(set(candidates))
            path = local[-1] if local else self._find(folders, exe_name)
            if path:
                found.append((label, path, generation))
        return found

    def locate(self, root):
        found = self.installed(root)
        return found[0][1] if found else None

    # -- launching -------------------------------------------------------

    def quick_actions(self, root):
        """One button per installed version, each opening that Cinebench."""
        found = self.installed(root)
        if not found:
            return [("Start", self.quick_config(root))]
        return [(label, dict(self.quick_config(root), version=label))
                for label, _path, _generation in found]

    def quick_summary(self, root):
        labels = [label for label, _p, _g in self.installed(root)]
        if not labels:
            return "None found"
        return "Opens " + " / ".join(labels)

    def build(self, config, root):
        found = {label: path for label, path, _gen in self.installed(root)}
        if not found:
            raise ToolUnavailable(
                "No Cinebench installation was found. Expected a folder "
                "beside this program, or an installed copy."
            )

        wanted = str(config.get("version", ""))
        if wanted not in found:
            wanted = sorted(found)[0]
        exe = found[wanted]

        # Opened, not driven. Which test a version offers and what it calls
        # them differs between R15 and R26, and the switches that started a
        # run from outside only ever fitted some of them -- so the run is
        # started in the window, where the choices are the ones that version
        # actually has.
        #
        # stdout is still read: Cinebench prints its result there even when
        # the run was started by hand, so "Rendering (Multiple CPU) : 906.43
        # pts" reaches the log rather than vanishing with the window. No
        # completion pattern, though -- treating the first score as the end
        # of the run would close it under somebody in the middle of a second.
        return LaunchSpec(
            argv=[exe],
            cwd=os.path.dirname(exe),
            console=True,
            error_key=self.key,
            summary="Cinebench " + wanted + " (run it in its own window)",
            duration_seconds=int(config.get("duration", 0)) * 60,
            leave_open=True,
            creation_flags=self._no_window_flags(),
        )
