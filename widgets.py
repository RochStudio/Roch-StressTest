"""The small widgets every tool panel is built out of.

Each of these takes theme colours from ``theme`` rather than being handed a
palette, so a colour is changed in one place and the whole program follows.
The point of the file is that a tool panel should be a list of fields, not a
hundred lines of grid arithmetic.
"""

import customtkinter as ctk

import theme


def section(parent, title):
    """A titled block with a rule under the heading.

    Returns the frame the caller should put rows into, not the outer frame --
    every caller wants the body, and handing back the wrapper just means
    every caller then reaches into it.
    """
    wrapper = ctk.CTkFrame(parent, corner_radius=6, fg_color=theme.SECTION_COLOR)
    wrapper.pack(fill="x", padx=6, pady=(0, 6))

    header = ctk.CTkLabel(
        wrapper,
        text=title,
        font=theme.HEADER_FONT,
        text_color=theme.TEXT_COLOR,
        anchor="w",
    )
    header.pack(fill="x", padx=10, pady=(6, 2))

    rule = ctk.CTkFrame(wrapper, height=1, fg_color=theme.RULE_COLOR)
    rule.pack(fill="x", padx=10, pady=(0, 6))

    body = ctk.CTkFrame(wrapper, fg_color="transparent")
    body.pack(fill="x", padx=10, pady=(0, 8))
    body.grid_columnconfigure(1, weight=1)
    return body


def label(parent, text, row, column=0, bold=False, colour=None, **grid):
    widget = ctk.CTkLabel(
        parent,
        text=text,
        font=theme.COMPACT_BOLD if bold else theme.COMPACT_FONT,
        text_color=colour or theme.TEXT_COLOR,
        anchor="w",
        justify="left",
    )
    widget.grid(row=row, column=column, sticky="w", pady=1, **grid)
    return widget


def hint(parent, text, row, column=1, span=2):
    """Grey explanatory text under a field.

    Wrapped rather than truncated: these say why a setting exists, and half a
    sentence is worse than none.
    """
    widget = ctk.CTkLabel(
        parent,
        text=text,
        font=(theme.FONT_FAMILY, 10),
        text_color=theme.SUBTITLE_COLOR,
        anchor="w",
        justify="left",
        wraplength=520,
    )
    widget.grid(row=row, column=column, columnspan=span, sticky="w",
                pady=(0, 4))
    return widget


class FieldRow:
    """One editable setting: a label, a control, and an optional unit.

    ``value()`` always returns something the tool's ``coerce`` will accept, so
    a panel can read every field without checking any of them.
    """

    def __init__(self, parent, field, row, on_change=None):
        self.field = field
        self.on_change = on_change

        label(parent, field.label, row)

        if field.kind == "bool":
            self.variable = ctk.BooleanVar(value=bool(field.default))
            self.widget = ctk.CTkSwitch(
                parent,
                text="",
                variable=self.variable,
                width=44,
                progress_color=theme.TAB_SELECTED_COLOR,
                command=self._changed,
            )
            self.widget.grid(row=row, column=1, sticky="w", padx=(8, 0), pady=1)
        elif field.kind == "choice":
            self.variable = ctk.StringVar(
                value=field.default or (field.choices[0] if field.choices else "")
            )
            self.widget = ctk.CTkOptionMenu(
                parent,
                variable=self.variable,
                values=field.choices or [""],
                width=theme.FIELD_WIDTH + 60,
                height=24,
                font=theme.COMPACT_FONT,
                dropdown_font=theme.COMPACT_FONT,
                fg_color=theme.TAB_UNSELECTED_COLOR,
                button_color=theme.TAB_UNSELECTED_COLOR,
                button_hover_color=theme.TAB_UNSELECTED_HOVER_COLOR,
                text_color=theme.TEXT_COLOR,
                command=lambda _value: self._changed(),
            )
            self.widget.grid(row=row, column=1, sticky="w", padx=(8, 0), pady=1)
        else:
            self.variable = ctk.StringVar(value=str(field.default))
            # Numbers need room for six digits; free text holds things like a
            # KMP_AFFINITY string, which is unreadable at the numeric width.
            self.widget = ctk.CTkEntry(
                parent,
                textvariable=self.variable,
                width=(theme.FIELD_WIDTH + 190 if field.kind == "text"
                       else theme.FIELD_WIDTH),
                height=22,
                font=theme.COMPACT_FONT,
                fg_color=theme.BG_COLOR2,
                border_color=theme.BORDER_COLOR,
                text_color=theme.TEXT_COLOR,
            )
            self.widget.grid(row=row, column=1, sticky="w", padx=(8, 0), pady=1)
            self.variable.trace_add("write", lambda *_: self._changed())

        if field.unit:
            label(parent, field.unit, row, column=2, colour=theme.SUBTITLE_COLOR,
                  padx=(6, 0))

    def _changed(self):
        if self.on_change:
            self.on_change(self.field.key)

    def value(self):
        return self.field.coerce(self.variable.get())

    def set(self, value):
        if self.field.kind == "bool":
            self.variable.set(bool(value))
        else:
            self.variable.set(str(value))

    def set_choices(self, choices, selected=None):
        if self.field.kind != "choice":
            return
        self.field.choices = list(choices)
        self.widget.configure(values=list(choices) or [""])
        if selected is not None:
            self.variable.set(selected)
        elif choices and self.variable.get() not in choices:
            self.variable.set(choices[0])


def action_button(parent, text, command, kind="normal", width=110):
    """A button in one of the three roles this program has for one."""
    colours = {
        "start": (theme.START_COLOR, theme.START_HOVER_COLOR),
        "stop": (theme.STOP_COLOR, theme.STOP_HOVER_COLOR),
        "normal": (theme.TAB_UNSELECTED_COLOR, theme.TAB_UNSELECTED_HOVER_COLOR),
    }[kind]
    return ctk.CTkButton(
        parent,
        text=text,
        command=command,
        width=width,
        height=26,
        corner_radius=6,
        font=theme.COMPACT_BOLD,
        fg_color=colours[0],
        hover_color=colours[1],
        text_color=("#FFFFFF" if kind != "normal" else theme.TEXT_COLOR),
    )
