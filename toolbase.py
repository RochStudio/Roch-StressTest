"""What every stress tool has in common, and the shape of a launch.

Each tool in this program is a small adapter: it says where its executable
lives, what can be configured about it, and how a set of configured values
turns into a process to start. Everything after that -- running it, timing it,
watching it for failures, stopping it -- is the runner's job and is written
once.

The adapters are deliberately thin. None of them reimplements a tool; they
write the configuration file or command line the tool already understands, and
that is all. Where a tool cannot be driven from the command line at all (RAM
Test Pro has no switches, TM5 has one argument), the adapter says so and the
UI tells the truth about it rather than pretending.
"""

import os
import shutil
import subprocess


class Field:
    """One configurable setting, and how to draw it.

    ``kind`` is one of "int", "text", "choice" or "bool". A field is the only
    thing the UI needs to build a row, so adding a setting to a tool means
    adding a Field and reading its key in ``build`` -- never touching the
    window code.
    """

    def __init__(
        self,
        key,
        label,
        kind="int",
        default=0,
        choices=None,
        hint="",
        minimum=None,
        maximum=None,
        unit="",
    ):
        self.key = key
        self.label = label
        self.kind = kind
        self.default = default
        self.choices = list(choices or [])
        self.hint = hint
        self.minimum = minimum
        self.maximum = maximum
        self.unit = unit

    def coerce(self, raw):
        """Turn a string from an entry box into the value ``build`` expects.

        Out-of-range and unparseable values fall back to the default rather
        than raising: a typo in a memory box should not stop a run that is
        about to be left alone for six hours.
        """
        if self.kind == "int":
            try:
                value = int(str(raw).strip())
            except (TypeError, ValueError):
                return self.default
            if self.minimum is not None:
                value = max(self.minimum, value)
            if self.maximum is not None:
                value = min(self.maximum, value)
            return value
        if self.kind == "bool":
            if isinstance(raw, bool):
                return raw
            return str(raw).strip().lower() in ("1", "true", "yes", "on")
        if self.kind == "choice":
            text = str(raw)
            return text if text in self.choices else self.default
        return str(raw)


class Preset:
    """A named set of field values, plus why you would pick it."""

    def __init__(self, name, values, description=""):
        self.name = name
        self.values = dict(values)
        self.description = description


class LaunchSpec:
    """Everything the runner needs to start and supervise one test."""

    def __init__(
        self,
        argv,
        cwd,
        env=None,
        console=False,
        watch_files=(),
        error_key="",
        summary="",
        duration_seconds=0,
        creation_flags=0,
    ):
        self.argv = list(argv)
        self.cwd = cwd
        self.env = env
        # True when the child writes to a pipe we can read. The windowed
        # tools do not, and are watched through their log files instead.
        self.console = console
        self.watch_files = list(watch_files)
        self.error_key = error_key
        self.summary = summary
        # 0 means "until stopped". The runner enforces this itself, because
        # only y-cruncher and Linpack can limit their own run time.
        self.duration_seconds = duration_seconds
        self.creation_flags = creation_flags


class ToolUnavailable(Exception):
    """Raised by ``build`` when the tool's files are not where they should be."""


class Tool:
    """Base class for a stress tool adapter."""

    key = ""
    name = ""
    blurb = ""
    # Relative to the tools root: the first path that exists wins, so a
    # version bump that renames the folder only needs a pattern added.
    exe_globs = ()
    # False for the windowed tools, whose output cannot be piped.
    console = False
    # Set on tools whose own window is the only place errors appear.
    detection_note = ""
    fields = ()
    presets = ()

    def locate(self, root):
        """Return the tool's executable under *root*, or None.

        Matching is by glob so an unpacked folder keeps its version in the
        name -- "p95v3019b20.win64" today, something else next release --
        without this file needing to know the number.
        """
        import glob

        for pattern in self.exe_globs:
            matches = sorted(glob.glob(os.path.join(root, pattern)))
            for match in matches:
                if os.path.isfile(match):
                    return match
        return None

    def available(self, root):
        return self.locate(root) is not None

    def defaults(self):
        """The field defaults as a plain dict."""
        return {field.key: field.default for field in self.fields}

    def preset_named(self, name):
        for preset in self.presets:
            if preset.name == name:
                return preset
        return None

    def build(self, config, root):
        """Turn configured values into a LaunchSpec. Implemented per tool."""
        raise NotImplementedError

    # -- helpers shared by the adapters ----------------------------------

    @staticmethod
    def _write(path, text):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8", newline="\r\n") as handle:
            handle.write(text)
        return path

    @staticmethod
    def _no_window_flags():
        """Start a console child without flashing a console window.

        CREATE_NO_WINDOW keeps the child's output on our pipe and off the
        screen, which is what we want for the tools we read directly.
        """
        return getattr(subprocess, "CREATE_NO_WINDOW", 0)

    @staticmethod
    def _copy_if_missing(source, destination):
        if os.path.isfile(source) and not os.path.isfile(destination):
            os.makedirs(os.path.dirname(destination), exist_ok=True)
            shutil.copy2(source, destination)
