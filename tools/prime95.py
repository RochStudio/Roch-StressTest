"""Prime95's torture test, driven from its own configuration file.

Prime95 has exactly two command-line switches worth anything here: ``-t`` to
start the torture test and ``-W`` to point it at a working directory. Every
setting the torture dialog offers is a key in ``prime.txt`` in that directory,
so the adapter writes that file and hands over.

Most references, including Prime95's own undoc.txt, put the torture keys in
``local.txt``. In 30.19 that is a migration path rather than where they live:
give it a local.txt and it folds those keys into prime.txt, deletes local.txt,
and carries on. Writing prime.txt directly skips a step that only ever half
happened, and it is verifiable -- ask for MinTortureFFT=MaxTortureFFT=1024 and
results.txt says "Self-test 1024K passed!" and nothing else.

Pointing ``-W`` at our own scratch directory rather than the distribution
folder matters more than it looks. Prime95 writes prime.txt, results.txt and
worker save files wherever it is told to work; sending them to LOCALAPPDATA
leaves the unpacked p95 folder byte-identical to what was downloaded, and
gives results.txt a stable path to watch for failures.
"""

import os

from core import settings
from core.toolbase import Field, LaunchSpec, Tool, ToolUnavailable


class Prime95(Tool):
    key = "prime95"
    name = "Prime95"
    blurb = (
        "FFT torture test. Smallest and Small FFTs are the heat and power "
        "test; Large, Blend and Huge lean on the memory controller and RAM."
    )
    exe_globs = ("p95*/prime95.exe", "prime95*/prime95.exe", "prime95.exe")
    console = False
    detection_note = (
        "Failures are read from results.txt, which Prime95 writes on every "
        "error, so a worker that fails while you are away is still caught."
    )

    fields = (
        Field("duration", "Stop after", "int", 0, minimum=0, maximum=100000,
              unit="min",
              hint="0 lets whatever you chose in Prime95's dialog run until "
                   "you press Stop."),
    )

    # Configured in Prime95's own window, so there is no tab here. Its torture
    # dialog cannot be preset from a file -- it opens on Blend whatever
    # prime.txt holds, and resets to Blend every time it is opened, even
    # mid-run -- so a tab of FFT sizes and memory figures could never be shown
    # to agree with what Prime95 was actually going to do. Choosing the test
    # in the one place that decides it is the honest arrangement.
    has_tab = False

    presets = ()

    quick_start = {
        "values": {"duration": 0},
    }

    def quick_actions(self, root):
        """One button, and it opens the tool rather than starting a run."""
        return [("Open", self.quick_config(root))]

    def quick_summary(self, root):
        """The card has no preset to name, so it says what will happen."""
        limit = int(self.quick_config(root).get("duration", 0) or 0)
        return "Opens its torture dialog" + (
            "  |  " + str(limit) + " min" if limit else "")

    def build(self, config, root):
        exe = self.locate(root)
        if not exe:
            raise ToolUnavailable(
                "prime95.exe was not found. Expected a folder like "
                "p95v3019b20.win64 beside this program."
            )

        work = settings.run_dir("prime95")

        # Four keys, and no torture settings at all. StressTester=1 is the
        # "just stress testing" answer that stops the GIMPS welcome dialog
        # waiting for somebody, and it is also what makes Prime95 raise its
        # torture dialog on startup -- which is the point here. The other two
        # mark the file as already migrated, suppressing the version prompt.
        self._write(os.path.join(work, "prime.txt"), "\n".join([
            "StressTester=1",
            "UsePrimenet=0",
            "V24OptionsConverted=1",
            "WGUID_version=2",
        ]) + "\n")

        # A local.txt left over from an earlier version of this program would
        # be migrated on top of what was just written.
        try:
            stale = os.path.join(work, "local.txt")
            if os.path.exists(stale):
                os.remove(stale)
        except OSError:
            pass

        # Started fresh each run so the failure watcher never reports an
        # error left over from the last one.
        results = os.path.join(work, "results.txt")
        try:
            if os.path.exists(results):
                os.remove(results)
        except OSError:
            pass

        # No -t: that would start a torture test from the file before there
        # was a chance to choose one.
        return LaunchSpec(
            argv=[exe, "-W" + work],
            cwd=os.path.dirname(exe),
            console=False,
            watch_files=[results],
            error_key=self.key,
            summary="Prime95 (test chosen in its own dialog)",
            duration_seconds=int(config.get("duration", 0)) * 60,
        )
