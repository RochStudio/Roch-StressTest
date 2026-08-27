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

import hardware
import runner as runner_module
import settings
import theme
import toolset
import widgets
from version import APP_NAME, __version__

# How often the UI drains the runner's event queue. Fast enough that output
# feels live, slow enough to be free.
PUMP_MS = 120

# The log keeps this many lines. A twelve-hour Linpack run produces far more
# than a text widget can redraw, and only the tail is ever read.
LOG_LIMIT = 4000

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
        from toolbase import Field

        body = widgets.section(parent, "What to run")
        names = [preset.name for preset in self.presets()]
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

    def _build_detection_section(self, parent):
        if not self.tool.detection_note:
            return
        body = widgets.section(parent, "Failure detection")
        widgets.hint(body, self.tool.detection_note, 0, column=0, span=3)

    def _build_actions(self, parent):
        bar = ctk.CTkFrame(parent, fg_color=theme.SECTION_COLOR,
                           corner_radius=6, height=42)
        bar.pack(side="bottom", fill="x", padx=6, pady=(6, 6))
        widgets.action_button(
            bar, "Start", self.start, kind="start", width=120
        ).pack(side="left", padx=(10, 0), pady=8)
        widgets.action_button(
            bar, "Add to queue", self.add_to_queue, width=130
        ).pack(side="left", padx=(8, 0), pady=8)

    # -- behaviour -------------------------------------------------------

    def apply_quick_start(self):
        """Open this tab on the tool's default configuration.

        The same values the Quick Start page runs, so the two never disagree
        about what "default" means. Pressing Start here without touching
        anything does exactly what the Quick Start button does.
        """
        name = self.tool.quick_preset_name(self.app.root_path)
        if name:
            self.preset_row.set(name)
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

        self.preset_note.configure(text=preset.description)
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
                if preset.values:
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
        preset = self.preset_row.value()
        return self.tool.name + " -- " + preset

    def start(self):
        self.app.start_single(self.tool, self.config(), self.label())

    def add_to_queue(self):
        self.app.add_to_queue(self.tool, self.config(), self.label())


class StressApp:
    def __init__(self, root):
        self.root = root
        self.root_path = toolset.tools_root()
        self.runner = runner_module.Runner()
        self.sequence = runner_module.Sequence(self.runner)
        self.panels = {}
        self.queue_steps = []
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
        width = min(880, max(760, screen_width - 80))
        height = min(880, max(640, screen_height - 120))
        x = max(0, (screen_width - width) // 2)
        y = max(0, (screen_height - height) // 2)
        self.root.geometry("{}x{}+{}+{}".format(width, height, x, y))
        self.root.minsize(760, 620)

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
            bar, text=APP_NAME, font=theme.TITLE_FONT,
            text_color=theme.TEXT_COLOR, anchor="w",
        ).pack(side="left", padx=(10, 6), pady=3)

        ctk.CTkLabel(
            bar, text=hardware.describe(), font=theme.COMPACT_FONT,
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
            tab = self.tabview.add(tool.name)
            if tool.available(self.root_path):
                self.panels[tool.key] = ToolPanel(self, tab, tool)
            else:
                self._build_missing_panel(tab, tool)

        self._build_queue_tab(self.tabview.add("Queue"))
        self._build_log_tab(self.tabview.add("Log"))
        self.tabview.set("Quick Start")

    def _build_quick_tab(self, parent):
        """One card per tool: what its default runs, and a button to run it.

        The whole page is a shortcut past the other tabs. Each card starts the
        tool's default configuration -- the same one its own tab opens on --
        so nothing here needs reading before it can be used.
        """
        bar = ctk.CTkFrame(parent, fg_color=theme.SECTION_COLOR,
                           corner_radius=6, height=42)
        bar.pack(side="bottom", fill="x", padx=6, pady=(6, 6))
        widgets.action_button(
            bar, "Queue all", self.queue_all_quick, width=120
        ).pack(side="left", padx=(10, 0), pady=8)
        ctk.CTkLabel(
            bar,
            text="Adds every test below to the queue, in this order.",
            font=(theme.FONT_FAMILY, 10), text_color=theme.SUBTITLE_COLOR,
            anchor="w",
        ).pack(side="left", padx=(10, 0), pady=8)

        frame = ctk.CTkScrollableFrame(parent, corner_radius=0,
                                       fg_color=theme.BG_COLOR)
        frame.pack(fill="both", expand=True)

        ctk.CTkLabel(
            frame,
            text=("Each button runs that tool's default configuration. The "
                  "tool's own tab opens on the same settings, so change them "
                  "there if you want something else."),
            font=theme.COMPACT_FONT, text_color=theme.SUBTITLE_COLOR,
            anchor="w", justify="left", wraplength=680,
        ).pack(fill="x", padx=12, pady=(10, 8))

        for tool in toolset.TOOLS:
            self._build_quick_card(frame, tool)

    def _build_quick_card(self, parent, tool):
        available = tool.available(self.root_path)
        body = widgets.section(parent, tool.name)

        ctk.CTkLabel(
            body,
            text=(tool.quick_summary(self.root_path) if available
                  else "Not found -- see the " + tool.name + " tab."),
            font=theme.COMPACT_BOLD,
            text_color=theme.TEXT_COLOR if available else theme.FAIL_COLOR,
            anchor="w", justify="left",
        ).grid(row=0, column=0, columnspan=2, sticky="w")

        note = tool.quick_note() if available else ""
        if note:
            widgets.hint(body, note, 1, column=0, span=2)

        buttons = ctk.CTkFrame(body, fg_color="transparent")
        buttons.grid(row=2, column=0, sticky="w", pady=(4, 0))
        start = widgets.action_button(
            buttons, "Start", lambda t=tool: self.start_quick(t),
            kind="start", width=110,
        )
        start.pack(side="left")
        add = widgets.action_button(
            buttons, "Add to queue", lambda t=tool: self.add_quick(t), width=130
        )
        add.pack(side="left", padx=(8, 0))
        if not available:
            start.configure(state="disabled")
            add.configure(state="disabled")

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

    def _build_queue_tab(self, parent):
        frame = ctk.CTkFrame(parent, fg_color=theme.BG_COLOR)
        frame.pack(fill="both", expand=True)

        ctk.CTkLabel(
            frame,
            text=("Tests run top to bottom, and the first failure ends the "
                  "run. Give every step a time limit -- a step set to 0 "
                  "never finishes on its own and the queue will stop there."),
            font=theme.COMPACT_FONT, text_color=theme.SUBTITLE_COLOR,
            anchor="w", justify="left", wraplength=700,
        ).pack(fill="x", padx=12, pady=(10, 8))

        self.queue_box = ctk.CTkTextbox(
            frame, font=theme.COMPACT_FONT, fg_color=theme.BG_COLOR2,
            border_color=theme.BORDER_COLOR, border_width=1,
            text_color=theme.TEXT_COLOR, activate_scrollbars=True,
        )
        self.queue_box.pack(fill="both", expand=True, padx=12, pady=(0, 8))
        self.queue_box.configure(state="disabled")

        bar = ctk.CTkFrame(frame, fg_color="transparent")
        bar.pack(fill="x", padx=12, pady=(0, 12))
        widgets.action_button(bar, "Run queue", self.start_queue,
                              kind="start", width=120).pack(side="left")
        widgets.action_button(bar, "Remove last", self.remove_last,
                              width=110).pack(side="left", padx=(8, 0))
        widgets.action_button(bar, "Clear", self.clear_queue,
                              width=80).pack(side="left", padx=(8, 0))
        self.refresh_queue()

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

        self.stop_button = widgets.action_button(
            strip, "Stop", self.stop_all, kind="stop", width=90
        )
        self.stop_button.pack(side="right", padx=(4, 10), pady=4)
        self.stop_button.configure(state="disabled")

        self.clock_label = ctk.CTkLabel(
            strip, text="", font=theme.STATUS_FONT,
            text_color=theme.TEXT_COLOR, anchor="e",
        )
        self.clock_label.pack(side="right", padx=(4, 8), pady=4)

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
        return self.runner.running or self.sequence.running

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
        self.log("Command: " + " ".join(spec.argv))
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
        self.stop_button.configure(state="normal")

    def quick_label(self, tool):
        return tool.name + " -- " + tool.quick_preset_name(self.root_path)

    def start_quick(self, tool):
        """Run a tool's default configuration straight from the Quick Start page."""
        self.start_single(tool, tool.quick_config(self.root_path),
                          self.quick_label(tool))

    def add_quick(self, tool):
        self.add_to_queue(tool, tool.quick_config(self.root_path),
                          self.quick_label(tool))

    def queue_all_quick(self):
        """Queue every available tool's default, in tab order."""
        for tool in toolset.TOOLS:
            if tool.available(self.root_path):
                self.queue_steps.append((tool, tool.quick_config(self.root_path),
                                         self.quick_label(tool)))
        self.refresh_queue()
        self.tabview.set("Queue")

    def add_to_queue(self, tool, config, label):
        self.queue_steps.append((tool, dict(config), label))
        self.refresh_queue()
        self.tabview.set("Queue")

    def remove_last(self):
        if self.queue_steps:
            self.queue_steps.pop()
            self.refresh_queue()

    def clear_queue(self):
        self.queue_steps = []
        self.refresh_queue()

    def refresh_queue(self):
        self.queue_box.configure(state="normal")
        self.queue_box.delete("1.0", "end")
        if not self.queue_steps:
            self.queue_box.insert(
                "end",
                "The queue is empty.\n\n"
                "Set a test up on its own tab, then press \"Add to queue\".\n"
                "A useful overnight run is TestMem5 Extreme, then y-cruncher, "
                "then Linpack -- memory first, because a memory fault will "
                "fail the CPU tests too and send you looking in the wrong "
                "place.",
            )
        else:
            total = 0
            for index, (_tool, config, label) in enumerate(self.queue_steps, 1):
                minutes = int(config.get("duration", 0))
                total += minutes
                limit = (str(minutes) + " min") if minutes else "no limit"
                self.queue_box.insert(
                    "end", "{:>2}. {:<44} {}\n".format(index, label, limit)
                )
            self.queue_box.insert(
                "end", "\nTotal: " + format_duration(total * 60)
                + (" (plus any step with no limit)" if any(
                    not step[1].get("duration") for step in self.queue_steps)
                   else "")
            )
        self.queue_box.configure(state="disabled")

    def start_queue(self):
        if self.busy():
            self.log("Already running. Stop the current test first.")
            return
        if not self.queue_steps:
            self.log("The queue is empty.")
            return
        self.log("")
        self.log("=== Queue: " + str(len(self.queue_steps)) + " steps ===")
        self.sequence.start(self.queue_steps, self.root_path)
        self.stop_button.configure(state="normal")

    def stop_all(self):
        if self.sequence.running:
            self.sequence.stop()
        else:
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
        elif kind == "step":
            self.current_label = data.get("label", "")
            if data.get("state") == runner_module.RUNNING:
                self.log("")
                self.log("=== Step {}/{}: {} ===".format(
                    data["index"] + 1, data["total"], data["label"]))
        elif kind == "queue-done":
            self._on_queue_done(data.get("results", []))

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
            self.stop_button.configure(state="normal")
            return

        self.log(STATE_TEXT.get(state, state).upper() + ": " + note)
        if self.stat_text:
            self.log("  " + self.stat_text)

        if state == runner_module.FAILED:
            self._announce_failure(note)

        # A queue keeps the Stop button live until the whole sequence ends.
        if not self.sequence.running:
            self.stop_button.configure(state="disabled")
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

    def _on_queue_done(self, results):
        self.log("")
        self.log("=== Queue finished ===")
        for label, state, note in results:
            line = "  " + STATE_TEXT.get(state, state).ljust(16) + label
            if note and state == runner_module.FAILED:
                line += "  -- " + note
            self.log(line)
        if results and all(state == runner_module.PASSED
                           for _label, state, _note in results):
            self._set_state(runner_module.PASSED,
                            "Every step passed: " + str(len(results)) + " of "
                            + str(len(results)) + ".")
        self.stop_button.configure(state="disabled")
        self.clock_label.configure(text="")

    def on_close(self):
        """Never leave a stress test running with no window to stop it."""
        if self.busy():
            self.sequence.stop()
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
    better than each tool failing differently halfway through a queue.
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
