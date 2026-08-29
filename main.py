"""Roch StressTest -- one window for every stress test on this machine.

The program is a launcher and a supervisor. It does not implement a single
test: Prime95, y-cruncher, TestMem5, RAM Test Pro and Linpack all do their own
work exactly as they would if you started them yourself. What it adds is the
part those tools leave to you -- knowing what each preset actually does,
starting them with settings that suit this machine rather than a default from
2009, holding them to a time limit, watching their output for a failure, and
chaining several into one unattended stability run.

Everything that touches a widget happens on the Tk thread. The runner works on
its own thread and reports through a queue that ``_pump_events`` drains on a
timer; that boundary is the only threading rule in the program, and it is why
a six-hour run does not deadlock at hour five.
"""

import ctypes
import os
import sys
import time
import queue as queue_module

import customtkinter as ctk

from core import hardware
from core import memory as memory_module
from core import runner as runner_module
from core import settings
from app import theme
import tools as toolset
from app import widgets
from core.version import APP_NAME, __version__

# How often the UI drains the runner's event queue. Fast enough that output
# feels live, slow enough to be free.
PUMP_MS = 120

# The log keeps this many lines. A twelve-hour Linpack run produces far more
# than a text widget can redraw, and only the tail is ever read.
LOG_LIMIT = 4000

# How often the live memory readout is refreshed.
RAM_MS = 1000

STATE_COLOURS = {
    runner_module.IDLE: theme.IDLE_COLOR,
    runner_module.RUNNING: theme.RUNNING_COLOR,
    runner_module.PASSED: theme.PASS_COLOR,
    runner_module.FAILED: theme.FAIL_COLOR,
    runner_module.STOPPED: theme.WARN_COLOR,
    runner_module.BROKEN: theme.FAIL_COLOR,
}

STATE_TEXT = {
    runner_module.IDLE: "Idle",
    runner_module.RUNNING: "Running",
    runner_module.PASSED: "Passed",
    runner_module.FAILED: "FAILED",
    runner_module.STOPPED: "Stopped",
    runner_module.BROKEN: "Could not start",
}


def format_duration(seconds):
    """Seconds as h:mm:ss, which is how a stress run is actually read."""
    seconds = int(max(0, seconds))
    hours, rest = divmod(seconds, 3600)
    minutes, secs = divmod(rest, 60)
    return "{}:{:02d}:{:02d}".format(hours, minutes, secs)


class ToolPanel:
    """One tool's tab: what it does, a preset picker, and its settings."""

    def __init__(self, app, parent, tool):
        self.app = app
        self.tool = tool
        self.rows = {}
        # Values for fields that have no widget, because the preset picker
        # already sets them. Without this the chosen TM5 profile would be
        # dropped between the picker and ``config``.
        self.hidden = {}
        # Set while a preset or a derived value is being written, so the
        # change callbacks those writes trigger do not recurse.
        self._loading = False

        # The action bar is packed first, against the bottom, so Start keeps
        # its place no matter how long the settings list grows. Putting it
        # inside the scrolling area buried it below the fold on every tool
        # with more than four settings.
        self._build_actions(parent)

        frame = ctk.CTkScrollableFrame(
            parent, corner_radius=0, fg_color=theme.BG_COLOR
        )
        frame.pack(fill="both", expand=True)
        self.frame = frame

        header = ctk.CTkLabel(
            frame,
            text=tool.blurb,
            font=theme.COMPACT_FONT,
            text_color=theme.SUBTITLE_COLOR,
            anchor="w",
            justify="left",
            wraplength=640,
        )
        header.pack(fill="x", padx=12, pady=(10, 8))

        self._build_unsupported_section(frame)
        self._build_preset_section(frame)
        self._build_settings_section(frame)
        self._build_detection_section(frame)

        self.apply_quick_start()

    # -- construction ----------------------------------------------------

    def presets(self):
        """Static presets, or ones discovered from the tool's own folder."""
        if hasattr(self.tool, "presets_for"):
            return self.tool.presets_for(self.app.root_path)
        return self.tool.presets

    def _build_preset_section(self, parent):
        from core.toolbase import Field

        names = [preset.name for preset in self.presets()]
        if not names:
            # Some tools have nothing to preset -- y-cruncher's algorithms
            # are ticked individually, and a picker with one entry would be
            # a control that does nothing.
            self.preset_row = None
            self.preset_note = None
            return

        body = widgets.section(parent, "What to run")
        field = Field("preset", "Preset", "choice", names[0] if names else "",
                      choices=names)
        self.preset_row = widgets.FieldRow(
            body, field, 0, on_change=lambda _key: self._preset_changed()
        )
        self.preset_note = widgets.hint(body, "", 1)

    def _build_settings_section(self, parent):
        body = widgets.section(parent, "Settings")
        row = 0
        skip = getattr(self.tool, "preset_field", None)
        for field in self.tool.fields:
            if field.key == skip:
                self.hidden[field.key] = field.default
                continue
            if field.kind == "choice" and not field.choices:
                # Filled in below from what is on disk; skipped entirely when
                # the tool's folder has nothing to offer.
                choices = self._dynamic_choices(field.key)
                if not choices:
                    continue
                field.choices = choices
                field.default = choices[0]
            self.rows[field.key] = widgets.FieldRow(
                body, field, row, on_change=self._field_changed
            )
            row += 1
            if field.hint:
                widgets.hint(body, field.hint, row)
                row += 1

    def _dynamic_choices(self, key):
        if key == "config" and hasattr(self.tool, "configs"):
            return [name for name, _ in self.tool.configs(self.app.root_path)]
        return []

    def _build_unsupported_section(self, parent):
        """A banner when the tool is present but cannot run on this machine."""
        reason = self.tool.unsupported_reason(self.app.root_path)
        if not reason:
            return
        body = widgets.section(parent, "Not for this processor")
        ctk.CTkLabel(
            body, text=reason, font=theme.COMPACT_FONT,
            text_color=theme.WARN_COLOR, anchor="w", justify="left",
            wraplength=880,
        ).grid(row=0, column=0, columnspan=3, sticky="w")

    def _build_detection_section(self, parent):
        if not self.tool.detection_note:
            return
        body = widgets.section(parent, "Failure detection")
        widgets.hint(body, self.tool.detection_note, 0, column=0, span=3)

    def _build_actions(self, parent):
        bar = ctk.CTkFrame(parent, fg_color=theme.SECTION_COLOR,
                           corner_radius=6, height=42)
        bar.pack(side="bottom", fill="x", padx=6, pady=(6, 6))
        start = widgets.action_button(
            bar, "Start", self.start, kind="start", width=120
        )
        start.pack(side="left", padx=(10, 0), pady=8)
        if self.tool.unsupported_reason(self.app.root_path):
            start.configure(state="disabled")

    # -- behaviour -------------------------------------------------------

    def apply_quick_start(self):
        """Open this tab on the tool's default configuration.

        The same values the Quick Start page runs, so the two never disagree
        about what "default" means. Pressing Start here without touching
        anything does exactly what the Quick Start button does.
        """
        name = self.tool.quick_preset_name(self.app.root_path)
        if name and self.preset_row is not None:
            self.preset_row.set(name)
        if self.preset_row is not None:
            self.apply_preset(name, initial=True)

        overrides = self.tool.quick_start.get("values", {})
        if not overrides:
            return
        self._loading = True
        try:
            for key, value in overrides.items():
                if key in self.rows:
                    self.rows[key].set(value)
                else:
                    self.hidden[key] = value
        finally:
            self._loading = False

    def _preset_changed(self):
        self.apply_preset(self.preset_row.value())

    def apply_preset(self, name, initial=False):
        preset = None
        for candidate in self.presets():
            if candidate.name == name:
                preset = candidate
                break
        if preset is None:
            return

        if self.preset_note is not None:
            self.preset_note.configure(text=preset.description)
        # Before the early return below: a preset with no values of its own
        # still decides which boxes it leaves you free to fill in.
        locked = set(self.tool.locked_fields(name))
        for key, row in self.rows.items():
            row.set_enabled(key not in locked)
        if not preset.values and not initial:
            return

        self._loading = True
        try:
            for key, value in preset.values.items():
                if key in self.rows:
                    self.rows[key].set(value)
                else:
                    self.hidden[key] = value
            # Memory is derived from the preset rather than stored in it, so
            # that the figure suits the machine it is running on today.
            if hasattr(self.tool, "suggested_memory") and "memory" in self.rows:
                if preset.values and "memory" not in locked:
                    self.rows["memory"].set(self.tool.suggested_memory(name))
            if hasattr(self.tool, "apply_memory") and preset.values:
                self._recompute_linpack()
        finally:
            self._loading = False

    def _field_changed(self, key):
        if self._loading:
            return
        if key == "memory" and hasattr(self.tool, "apply_memory"):
            self._recompute_linpack()

    def _recompute_linpack(self):
        """Keep problem size and leading dimension in step with memory."""
        was_loading = self._loading
        self._loading = True
        try:
            config = self.config()
            self.tool.apply_memory(config)
            for key in ("problem_size", "leading_dimension"):
                if key in self.rows:
                    self.rows[key].set(config[key])
        finally:
            self._loading = was_loading

    def config(self):
        values = dict(self.hidden)
        values.update({key: row.value() for key, row in self.rows.items()})
        return values

    def label(self):
        if self.preset_row is None:
            return self.tool.name
        return self.tool.name + " -- " + self.preset_row.value()

    def start(self):
        self.app.start_single(self.tool, self.config(), self.label())


class StressApp:
    def __init__(self, root):
        self.root = root
        self.root_path = toolset.tools_root()
        self.runner = runner_module.Runner()
        self.panels = {}
        self.log_lines = 0
        self.current_label = ""
        self.stat_text = ""

        self.root.title(APP_NAME + " " + __version__)
        self.set_window_icon()
        self.setup_appearance()
        self.create_widgets()
        self.setup_window_geometry()
        self.log(APP_NAME + " " + __version__)
        self.log(hardware.describe())
        self.report_missing()
        self.root.protocol("WM_DELETE_WINDOW", self.on_close)
        self.root.after(PUMP_MS, self._pump_events)
        self._update_ram()

    # -- chrome ----------------------------------------------------------

    def icon_path(self):
        path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "icon.ico")
        return path if os.path.exists(path) else None

    def set_window_icon(self):
        try:
            path = self.icon_path()
            if path:
                self.root.iconbitmap(path)
        except Exception as error:
            print("Error setting window icon: " + str(error))

    def setup_appearance(self):
        self.appearance_mode = settings.load_appearance_mode()
        ctk.set_appearance_mode(self.appearance_mode)
        ctk.set_default_color_theme("dark-blue")
        self.root.configure(fg_color=theme.BG_COLOR)

    def change_appearance_mode(self, mode):
        self.appearance_mode = str(mode).title()
        ctk.set_appearance_mode(self.appearance_mode)
        settings.save_setting("appearance_mode", self.appearance_mode)

    def setup_window_geometry(self):
        screen_width = self.root.winfo_screenwidth()
        screen_height = self.root.winfo_screenheight()
        # 850x775, and only smaller than that when the screen cannot hold it.
        width = min(850, max(700, screen_width - 80))
        height = min(775, max(560, screen_height - 120))
        x = max(0, (screen_width - width) // 2)
        y = max(0, (screen_height - height) // 2)
        self.root.geometry("{}x{}+{}+{}".format(width, height, x, y))
        self.root.minsize(700, 560)

    def create_widgets(self):
        self.main_frame = ctk.CTkFrame(
            self.root, corner_radius=0, fg_color=theme.BG_COLOR
        )
        self.main_frame.pack(fill="both", expand=True)

        self._build_toolbar()
        self._build_tabs()
        self._build_status_strip()

    def _build_toolbar(self):
        bar = ctk.CTkFrame(self.main_frame, corner_radius=0,
                           fg_color=theme.HEADER_COLOR, height=32)
        bar.pack(fill="x", padx=2, pady=(2, 2))

        ctk.CTkLabel(
            bar, text=hardware.describe_cpu(), font=theme.COMPACT_FONT,
            text_color=theme.SUBTITLE_COLOR, anchor="w",
        ).pack(side="left", padx=(4, 0), pady=3)

        self.appearance_menu = ctk.CTkOptionMenu(
            bar,
            values=["Dark", "Light"],
            command=self.change_appearance_mode,
            width=76, height=22,
            font=theme.COMPACT_BOLD, dropdown_font=theme.COMPACT_FONT,
            fg_color=theme.TAB_UNSELECTED_COLOR,
            button_color=theme.TAB_UNSELECTED_COLOR,
            button_hover_color=theme.TAB_UNSELECTED_HOVER_COLOR,
            text_color=theme.TEXT_COLOR,
        )
        self.appearance_menu.set(self.appearance_mode)
        self.appearance_menu.pack(side="right", padx=(4, 8), pady=3)

        widgets.action_button(
            bar, "Open logs", self.open_logs, width=90
        ).pack(side="right", padx=(4, 0), pady=3)

        # Memory, live, next to the button that frees it. A stress test only
        # tests the memory it can actually get: Windows counts the standby
        # list as used, so on a machine that has been up a while, asking for
        # 28 GB quietly gets part of it from the page file instead.
        self.clean_button = widgets.action_button(
            bar, "Clean memory", self.clean_memory, width=120
        )
        self.clean_button.pack(side="right", padx=(4, 0), pady=3)

        self.ram_label = ctk.CTkLabel(
            bar, text="", font=theme.STATUS_FONT,
            text_color=theme.TEXT_COLOR, anchor="e",
        )
        self.ram_label.pack(side="right", padx=(8, 6), pady=3)

    def _build_tabs(self):
        self.tabview = ctk.CTkTabview(
            self.main_frame,
            fg_color=theme.BG_COLOR,
            segmented_button_fg_color=theme.BG_COLOR2,
            segmented_button_selected_color=theme.TAB_SELECTED_COLOR,
            segmented_button_selected_hover_color=theme.TAB_HOVER_COLOR,
            segmented_button_unselected_color=theme.TAB_UNSELECTED_COLOR,
            segmented_button_unselected_hover_color=theme.TAB_UNSELECTED_HOVER_COLOR,
            corner_radius=8,
            border_width=0,
            height=36,
            anchor="w",
        )
        self.tabview._segmented_button.configure(
            corner_radius=8, border_width=1,
            fg_color=theme.BG_COLOR,
            selected_color=theme.TAB_SELECTED_COLOR,
            selected_hover_color=theme.TAB_HOVER_COLOR,
            unselected_color=theme.TAB_UNSELECTED_COLOR,
            unselected_hover_color=theme.TAB_UNSELECTED_HOVER_COLOR,
            text_color=theme.TEXT_COLOR,
            font=theme.TAB_FONT,
        )
        self.tabview._segmented_button._text_color = theme.TEXT_COLOR
        self.tabview._segmented_button._selected_text_color = theme.TEXT_COLOR
        self.tabview.pack(fill="both", expand=True, padx=2, pady=2)

        # First, because it is the answer to "I just want to run the thing".
        self._build_quick_tab(self.tabview.add("Quick Start"))

        for tool in toolset.TOOLS:
            # A tool configured in its own window has nothing to put on a tab.
            if not getattr(tool, "has_tab", True):
                continue
            tab = self.tabview.add(tool.name)
            if tool.available(self.root_path):
                self.panels[tool.key] = ToolPanel(self, tab, tool)
            else:
                self._build_missing_panel(tab, tool)

        self._build_log_tab(self.tabview.add("Log"))
        self.tabview.set("Quick Start")

    def _build_quick_tab(self, parent):
        """One card per tool: what its default runs, and a button to run it.

        The whole page is a shortcut past the other tabs. Each card starts the
        tool's default configuration -- the same one its own tab opens on --
        so nothing here needs reading before it can be used.
        """
        # Scrollable, but the bar is hidden while everything fits -- which is
        # the normal case, and a scrollbar sitting there doing nothing is the
        # thing this page can least afford. It reappears the moment the
        # content outgrows the window.
        frame = ctk.CTkScrollableFrame(parent, corner_radius=0,
                                       fg_color=theme.BG_COLOR)
        frame.pack(fill="both", expand=True)
        self._auto_hide_scrollbar(frame)

        ctk.CTkLabel(
            frame,
            text=("One test at a time. Close a tool's own window to end a run "
                  "it is driving."),
            font=theme.COMPACT_FONT, text_color=theme.SUBTITLE_COLOR,
            anchor="w", justify="left", wraplength=900,
        ).pack(fill="x", padx=8, pady=(4, 3))

        # Two columns of cards rather than one tall list, so all five tools
        # are on screen at once and Quick Start needs no scrolling to use.
        # The cards themselves still pack vertically -- each column is its
        # own frame -- which keeps the section widget unchanged.
        columns = ctk.CTkFrame(frame, fg_color="transparent")
        columns.pack(fill="both", expand=True)
        columns.grid_columnconfigure(0, weight=1, uniform="quick")
        columns.grid_columnconfigure(1, weight=1, uniform="quick")

        sides = []
        for index in range(2):
            side = ctk.CTkFrame(columns, fg_color="transparent")
            side.grid(row=0, column=index, sticky="new")
            sides.append(side)

        # The columns are laid out in toolset rather than split down the
        # middle of the tab order, so related tools sit together.
        for side, tools in zip(sides, toolset.quick_columns()):
            for tool in tools:
                self._build_quick_card(side, tool)

    @staticmethod
    def _auto_hide_scrollbar(frame):
        """Hide a scrollable frame's bar while its content fits.

        CustomTkinter always shows it. Reaching for the canvas and the bar
        behind the widget is the only way to ask whether scrolling is needed,
        so both are fetched defensively: if a later version renames either,
        the page keeps its scrollbar rather than failing to draw.
        """
        canvas = getattr(frame, "_parent_canvas", None)
        bar = getattr(frame, "_scrollbar", None)
        if canvas is None or bar is None:
            return

        def sync(_event=None):
            try:
                first, last = canvas.yview()
            except Exception:
                return
            if first <= 0.0 and last >= 1.0:
                bar.grid_remove()
            else:
                bar.grid()

        for widget in (frame, canvas):
            widget.bind("<Configure>", sync, add="+")
        frame.after(150, sync)

    def _build_quick_card(self, parent, tool):
        available = tool.available(self.root_path)
        body = widgets.section(parent, tool.name)

        blocked = tool.unsupported_reason(self.root_path) if available else ""
        if not available and not getattr(tool, "has_tab", True):
            # No tab to send anybody to, so the card has to say where it
            # should go instead.
            headline = "Not found -- unpack it into the program folder."
        elif not available:
            headline = "Not found -- see the " + tool.name + " tab."
        elif blocked:
            headline = "Not for this processor"
        else:
            headline = tool.quick_summary(self.root_path)

        if headline:
            ctk.CTkLabel(
                body, text=headline, font=theme.COMPACT_BOLD,
                text_color=(theme.TEXT_COLOR if available and not blocked
                            else theme.WARN_COLOR if blocked
                            else theme.FAIL_COLOR),
                anchor="w", justify="left", wraplength=400,
            ).grid(row=0, column=0, columnspan=2, sticky="w")

        note = blocked or (tool.quick_note() if available
                           else self.root_path if not getattr(
                               tool, "has_tab", True) else "")
        if note:
            widgets.hint(body, note, 1, column=0, span=2, wrap=380)

        buttons = ctk.CTkFrame(body, fg_color="transparent")
        buttons.grid(row=2, column=0, sticky="w", pady=(3, 0))

        actions = ([("Start", None)] if not available or blocked
                   else tool.quick_actions(self.root_path))

        # Four to a row. Cinebench has a button per version installed, which
        # is six on a machine with the lot, and in one row they ran off the
        # edge of the card with the last one unreachable.
        rows, per_row = [], 4
        for index in range(0, len(actions), per_row):
            row = ctk.CTkFrame(buttons, fg_color="transparent")
            row.pack(anchor="w", pady=(0, 3 if index + per_row < len(actions)
                                       else 0))
            rows.append(row)

        for position, (label, config) in enumerate(actions):
            holder = rows[position // per_row]
            if config is None:
                command = lambda t=tool: self.start_quick(t)
            else:
                command = (lambda t=tool, c=config, n=label:
                           self.start_single(t, c, t.name + " -- " + n))
            button = widgets.action_button(
                holder, label, command, kind="start",
                width=110 if len(actions) == 1 else 96,
            )
            button.pack(side="left", padx=(0, 4))
            if not available or blocked:
                button.configure(state="disabled")

    def _build_missing_panel(self, parent, tool):
        frame = ctk.CTkFrame(parent, fg_color=theme.BG_COLOR)
        frame.pack(fill="both", expand=True)
        ctk.CTkLabel(
            frame,
            text=tool.name + " was not found.",
            font=theme.HEADER_FONT, text_color=theme.FAIL_COLOR,
        ).pack(anchor="w", padx=16, pady=(20, 4))
        ctk.CTkLabel(
            frame,
            text=("Unpack it into\n" + self.root_path
                  + "\n\nExpected one of:\n  "
                  + "\n  ".join(tool.exe_globs)),
            font=theme.COMPACT_FONT, text_color=theme.SUBTITLE_COLOR,
            anchor="w", justify="left",
        ).pack(anchor="w", padx=16)

    def _build_log_tab(self, parent):
        frame = ctk.CTkFrame(parent, fg_color=theme.BG_COLOR)
        frame.pack(fill="both", expand=True)
        self.log_box = ctk.CTkTextbox(
            frame, font=theme.LOG_FONT, fg_color=theme.BG_COLOR2,
            border_color=theme.BORDER_COLOR, border_width=1,
            text_color=theme.TEXT_COLOR, wrap="none",
        )
        self.log_box.pack(fill="both", expand=True, padx=12, pady=(10, 8))
        self.log_box.configure(state="disabled")

        bar = ctk.CTkFrame(frame, fg_color="transparent")
        bar.pack(fill="x", padx=12, pady=(0, 12))
        widgets.action_button(bar, "Save transcript", self.save_log,
                              width=130).pack(side="left")
        widgets.action_button(bar, "Clear", self.clear_log,
                              width=80).pack(side="left", padx=(8, 0))

    def _build_status_strip(self):
        strip = ctk.CTkFrame(self.main_frame, corner_radius=0,
                             fg_color=theme.HEADER_COLOR, height=34)
        strip.pack(fill="x", padx=2, pady=(0, 2))

        self.state_label = ctk.CTkLabel(
            strip, text="Idle", font=theme.STATUS_FONT,
            text_color=theme.IDLE_COLOR, width=110, anchor="w",
        )
        self.state_label.pack(side="left", padx=(10, 6), pady=4)

        self.detail_label = ctk.CTkLabel(
            strip, text="Nothing running.", font=theme.COMPACT_FONT,
            text_color=theme.TEXT_COLOR, anchor="w",
        )
        self.detail_label.pack(side="left", padx=(0, 8), pady=4)


        self.clock_label = ctk.CTkLabel(
            strip, text="", font=theme.STATUS_FONT,
            text_color=theme.TEXT_COLOR, anchor="e",
        )
        self.clock_label.pack(side="right", padx=(4, 8), pady=4)

    # -- memory ----------------------------------------------------------

    def _update_ram(self):
        """Refresh the live memory readout once a second.

        Separate from the event pump, which runs eight times a second: the
        figure does not move that fast and there is no reason to redraw it
        that often.
        """
        try:
            available, total, used = memory_module.reading()
            self.ram_label.configure(
                text="RAM {:,} / {:,} MB free".format(available, total),
                # Red once memory is nearly gone, which during a 28 GB run is
                # the normal state and worth being able to see at a glance.
                text_color=(theme.FAIL_COLOR if used >= 90
                            else theme.WARN_COLOR if used >= 75
                            else theme.TEXT_COLOR),
            )
        finally:
            self.root.after(RAM_MS, self._update_ram)

    def clean_memory(self):
        """Empty working sets and purge the standby lists, and say what happened."""
        self.clean_button.configure(state="disabled", text="Cleaning...")
        self.root.update_idletasks()
        try:
            result = memory_module.clean()
        except Exception as error:
            self.log("Could not clean memory: " + str(error))
        else:
            self.log(result.describe())
            if result.failed and not result.done:
                self.log("  Memory cleaning needs administrator rights, "
                         "which this program normally asks for at launch.")
        finally:
            self.clean_button.configure(state="normal", text="Clean memory")
            self._update_ram_now()

    def _update_ram_now(self):
        available, total, _used = memory_module.reading()
        self.ram_label.configure(
            text="RAM {:,} / {:,} MB free".format(available, total))

    # -- log -------------------------------------------------------------

    def log(self, text):
        self.log_box.configure(state="normal")
        self.log_box.insert("end", text + "\n")
        self.log_lines += 1
        if self.log_lines > LOG_LIMIT:
            self.log_box.delete("1.0", str(self.log_lines - LOG_LIMIT) + ".0")
            self.log_lines = LOG_LIMIT
        self.log_box.see("end")
        self.log_box.configure(state="disabled")

    def clear_log(self):
        self.log_box.configure(state="normal")
        self.log_box.delete("1.0", "end")
        self.log_box.configure(state="disabled")
        self.log_lines = 0

    def save_log(self):
        path = os.path.join(
            settings.log_dir(),
            time.strftime("stress-%Y%m%d-%H%M%S.txt"),
        )
        try:
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(self.log_box.get("1.0", "end"))
            self.log("Transcript saved to " + path)
        except OSError as error:
            self.log("Could not save the transcript: " + str(error))

    def open_logs(self):
        try:
            os.startfile(settings.log_dir())
        except Exception as error:
            self.log("Could not open the log folder: " + str(error))

    def report_missing(self):
        for tool in toolset.missing(self.root_path):
            self.log("Not found: " + tool.name + " (its tab explains where "
                     "to unpack it)")

    # -- running ---------------------------------------------------------

    def busy(self):
        return self.runner.running

    def start_single(self, tool, config, label):
        if self.busy():
            self.log("Already running. Stop the current test first.")
            return
        try:
            spec = tool.build(config, self.root_path)
        except Exception as error:
            self.log("Could not start " + tool.name + ": " + str(error))
            self._set_state(runner_module.BROKEN, str(error))
            return
        self.current_label = label
        self.log("")
        self.log("=== " + label + " ===")
        self.log(spec.summary)
        # The raw command line when there is one, because for the two tools
        # that use it the argument list is not what actually gets run.
        self.log("Command: " + (spec.cmdline or " ".join(spec.argv)))
        if spec.duration_seconds:
            self.log("Time limit: " + format_duration(spec.duration_seconds))
        else:
            self.log("No time limit -- runs until you press Stop.")
        try:
            self.runner.start(spec, label)
        except Exception as error:
            self.log("Could not start the process: " + str(error))
            self._set_state(runner_module.BROKEN, str(error))
            return

    def quick_label(self, tool):
        name = tool.quick_preset_name(self.root_path)
        return tool.name + (" -- " + name if name else "")

    def start_quick(self, tool):
        """Run a tool's default configuration straight from the Quick Start page."""
        self.start_single(tool, tool.quick_config(self.root_path),
                          self.quick_label(tool))

    def stop_all(self):
        self.runner.stop()
        self.log("Stop requested.")

    # -- event pump ------------------------------------------------------

    def _pump_events(self):
        try:
            while True:
                try:
                    event = self.runner.events.get_nowait()
                except queue_module.Empty:
                    break
                self._handle(event)
        finally:
            self.root.after(PUMP_MS, self._pump_events)

    def _handle(self, event):
        kind = event.kind
        data = event.data
        if kind == "output":
            self.log("  " + data["line"])
        elif kind == "stat":
            self.stat_text = data["text"]
        elif kind == "tick":
            self._update_clock(data.get("elapsed", 0), data.get("remaining"))
        elif kind == "state":
            self._on_state(data["state"], data.get("note", ""))

    def _update_clock(self, elapsed, remaining):
        text = format_duration(elapsed)
        if remaining is not None:
            text += "  /  " + format_duration(remaining) + " left"
        self.clock_label.configure(text=text)
        if self.stat_text:
            self.detail_label.configure(
                text=self.current_label + "   " + self.stat_text)

    def _set_state(self, state, note=""):
        self.state_label.configure(
            text=STATE_TEXT.get(state, state),
            text_color=STATE_COLOURS.get(state, theme.TEXT_COLOR),
        )
        detail = note or self.current_label or "Nothing running."
        self.detail_label.configure(text=detail)

    def _on_state(self, state, note):
        self._set_state(state, note)
        if state == runner_module.RUNNING:
            self.stat_text = ""
            return

        self.log(STATE_TEXT.get(state, state).upper() + ": " + note)
        if self.stat_text:
            self.log("  " + self.stat_text)

        if state == runner_module.FAILED:
            self._announce_failure(note)

        self.clock_label.configure(text="")

    def _announce_failure(self, note):
        """Make a failure impossible to miss, and keep a copy of it.

        The transcript is written without being asked because the run that
        matters is the one that failed at 4am, and the window may well be
        gone by the time anybody looks.
        """
        try:
            self.root.bell()
        except Exception:
            pass
        path = os.path.join(
            settings.log_dir(), time.strftime("FAILED-%Y%m%d-%H%M%S.txt")
        )
        try:
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(APP_NAME + " " + __version__ + "\n")
                handle.write(hardware.describe() + "\n")
                handle.write(self.current_label + "\n")
                handle.write(note + "\n\n")
                handle.write(self.log_box.get("1.0", "end"))
            self.log("Transcript of the failure saved to " + path)
        except OSError:
            pass
        try:
            self.tabview.set("Log")
        except Exception:
            pass

    def on_close(self):
        """Never leave a stress test running with no window to stop it."""
        if self.busy():
            self.runner.stop()
        self.root.destroy()


def is_admin():
    """Check whether the current process has administrative privileges."""
    try:
        return bool(ctypes.windll.shell32.IsUserAnAdmin())
    except Exception:
        return False


def run_as_admin():
    """Relaunch with a UAC prompt.

    TestMem5 and RAM Test Pro need it to lock physical pages, and Prime95
    needs it to set process priority and affinity. Asking once here is far
    better than a tool failing halfway through a run for want of one.
    """
    parameters = None
    if not getattr(sys, "frozen", False):
        parameters = '"' + os.path.abspath(__file__) + '"'
    ctypes.windll.shell32.ShellExecuteW(
        None, "runas", sys.executable, parameters, None, 1
    )
    sys.exit(0)


if __name__ == "__main__":
    # --no-elevate is for looking at the window without a UAC prompt. The
    # tools themselves still need administrator rights, so a run started this
    # way will fail in the tool rather than here -- it is for working on the
    # interface, not for running tests.
    if "--no-elevate" not in sys.argv and not is_admin():
        print("Admin privileges required. Relaunching with UAC prompt...")
        run_as_admin()
    root = ctk.CTk()
    app = StressApp(root)
    if not is_admin():
        app.log("Running without administrator rights: TestMem5 and RAM Test "
                "Pro will not be able to lock memory.")
    root.mainloop()
