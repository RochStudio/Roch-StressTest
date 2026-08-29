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
        if self.kind == "multi":
            wanted = set(str(raw).replace(",", " ").split())
            return " ".join(c for c in self.choices if c in wanted)
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
        cmdline=None,
        completion_patterns=(),
        abort_patterns=(),
        leave_open=False,
        on_started=None,
        setup_patterns=(),
    ):
        # Lines that mean the tool could not run for a reason that has
        # nothing to do with the hardware -- a licence it does not have, a
        # setting it will not accept. Reported as "could not start", never as
        # a failure: telling somebody their memory is unstable because a
        # benchmark wanted a Professional licence would be a lie.
        self.setup_patterns = list(setup_patterns)
        # Called once, on the runner's thread, just after the process starts.
        # For the one tool that has no command line and no settings file, this
        # is where its window gets filled in and its Start button pressed. It
        # returns a line for the log, or raises -- and a run whose setup
        # raised is never started, because a stress test with settings other
        # than the ones asked for is worse than no test.
        self.on_started = on_started
        # Filled in by the runner just before on_started is called, so
        # a setup step can recognise its own process among the
        # windows on screen.
        self.started_pid = None
        # Leave the tool on screen once it has finished by itself, instead of
        # killing it the moment the verdict is known. This is the whole point
        # of y-cruncher's pause:1 -- it holds its window open showing the
        # result -- and killing the process is what made that option look
        # broken. Only applies to a run that ended on its own terms: a time
        # limit or a stop still has to actually stop it.
        self.leave_open = leave_open
        # Lines that mean the tool has finished its work, and lines that mean
        # it stopped without finishing. Needed for any tool that does not exit
        # when it is done: TestMem5 finishes its cycles, writes "Testing
        # completed", and leaves its window open indefinitely. Waiting for the
        # process to end would wait for ever, and a run with no time limit --
        # which is the right way to run a fixed number of cycles -- would
        # never be reported at all.
        self.completion_patterns = list(completion_patterns)
        self.abort_patterns = list(abort_patterns)
        self.argv = list(argv)
        # A raw command line, used instead of argv when set. Needed for one
        # tool only: TestMem5 takes `Config File="name.cfg"`, a parameter
        # whose *name* contains a space, and no list of arguments survives
        # being re-quoted into that shape. Everything else uses argv, which
        # cannot be mangled by quoting rules.
        self.cmdline = cmdline
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
    # Absolute patterns, searched after the tools root. Some of these tools
    # are installed rather than unpacked -- 3DMark lives under Program Files,
    # Cinebench under whichever launcher put it there -- and copying an
    # installed application into this folder to satisfy a lookup would be
    # worse than looking where it actually is.
    external_globs = ()
    # False for the windowed tools, whose output cannot be piped.
    console = False
    # Set on tools whose own window is the only place errors appear.
    detection_note = ""
    fields = ()
    presets = ()
    # The one configuration this tool should run when nobody has chosen
    # anything: {"preset": name, "values": {...}, "note": "..."}. It is what
    # the Quick Start page launches and what the tool's own tab opens on, so
    # there is exactly one default per tool and both places agree on it.
    quick_start = {}

    def locate(self, root):
        """Return the tool's executable under *root*, or None.

        Matching is by glob so an unpacked folder keeps its version in the
        name -- "p95v3019b20.win64" today, something else next release --
        without this file needing to know the number.
        """
        import glob

        for pattern in self.exe_globs:
            for match in sorted(glob.glob(os.path.join(root, pattern))):
                if os.path.isfile(match):
                    return match
        # Newest first, so a machine with several versions installed gets the
        # current one rather than whichever sorts first.
        for pattern in self.external_globs:
            matches = [m for m in glob.glob(pattern) if os.path.isfile(m)]
            if matches:
                return sorted(matches)[-1]
        return None

    def available(self, root):
        return self.locate(root) is not None

    def unsupported_reason(self, root):
        """Why this tool cannot run on this machine, or "" when it can.

        Separate from ``available``, which is about whether the files are
        there. A tool can be present and still be the wrong tool for the
        processor, and saying so beats letting it start and do nothing.
        """
        return ""

    def defaults(self):
        """The field defaults as a plain dict."""
        return {field.key: field.default for field in self.fields}

    def all_presets(self, root):
        """Static presets, or ones discovered from the tool's own folder."""
        if hasattr(self, "presets_for"):
            return self.presets_for(root)
        return self.presets

    def quick_preset_name(self, root):
        """The preset the quick-start default is built on, if any."""
        wanted = self.quick_start.get("preset")
        names = [preset.name for preset in self.all_presets(root)]
        if wanted in names:
            return wanted
        return names[0] if names else ""

    def quick_config(self, root):
        """The tool's default configuration, fully resolved.

        Built from the field defaults, then the named preset, then the
        quick-start overrides -- in that order, so an override always wins.
        Machine-dependent figures are recomputed here rather than stored,
        because a memory size that was right on the machine this was written
        on is not a default, it is a guess.
        """
        config = self.defaults()
        name = self.quick_preset_name(root)
        for preset in self.all_presets(root):
            if preset.name == name:
                config.update(preset.values)
                break
        if hasattr(self, "suggested_memory") and name:
            config["memory"] = self.suggested_memory(name)
        config.update(self.quick_start.get("values", {}))
        if hasattr(self, "apply_memory"):
            self.apply_memory(config)
        return config

    # Whether this tool gets a tab of its own. False for a tool that is
    # configured in its own window rather than here -- a tab of settings that
    # do not reach it would be a tab that lies.
    has_tab = True

    def locked_fields(self, preset_name):
        """Field keys this preset does not let the user set.

        A preset that is really the tool's own -- Prime95 derives its FFT
        ranges from the cache sizes it finds, so they are not the same on two
        different processors -- has no business offering an editable box whose
        value is then thrown away.
        """
        return ()

    def quick_actions(self, root):
        """The Quick Start buttons for this tool, as (label, config) pairs.

        One button running the default is the usual case. A tool overrides
        this when its card is worth more than that -- y-cruncher has a menu
        worth opening and a .bat or two beside it, and three buttons say that
        better than one button and a tab does.
        """
        return [("Start", self.quick_config(root))]

    def quick_note(self):
        return self.quick_start.get("note", "")

    def quick_summary(self, root):
        """A one-line description of the default, with no side effects.

        Deliberately not ``build(spec).summary``: building a spec writes
        configuration files, and the Quick Start page describes five tools the
        moment the window opens. Reaching for the real summary there would
        rewrite Prime95's prime.txt and TM5's MT.cfg before anybody had
        pressed anything.
        """
        config = self.quick_config(root)
        parts = [self.quick_preset_name(root)]
        cycles = int(config.get("cycles", 0) or 0)
        if cycles:
            parts.append(str(cycles) + (" cycles" if cycles != 1 else " cycle"))
        errors_limit = int(config.get("max_errors", 0) or 0)
        if errors_limit:
            parts.append("stop at " + str(errors_limit) + " error"
                         + ("s" if errors_limit != 1 else ""))
        parts.append(self.duration_note(config))
        return "  |  ".join(part for part in parts if part)

    def duration_note(self, config):
        """What the card should say about how this run ends.

        Overridable because the limit does not always apply: a single
        Cinebench render ends when the render ends and never consults it, and
        printing "30 min" beside it promises a soak that is not going to
        happen.
        """
        duration = int(config.get("duration", 0) or 0)
        return str(duration) + " min" if duration else "no time limit"

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
    def _new_console_flags():
        """Give a console child its own visible window.

        The windowed tools put themselves on screen; the two console ones
        would otherwise run entirely unseen. Output cannot be piped at the
        same time -- a redirected child writes to the pipe and its console
        stays blank -- so a tool shown this way is watched through a file
        instead, and each adapter arranges one.
        """
        return getattr(subprocess, "CREATE_NEW_CONSOLE", 0x00000010)

    @staticmethod
    def _no_window_flags():
        """Start a console child without flashing a console window.

        CREATE_NO_WINDOW keeps the child's output on our pipe and off the
        screen, which is what we want for the tools we read directly.
        """
        return getattr(subprocess, "CREATE_NO_WINDOW", 0)
