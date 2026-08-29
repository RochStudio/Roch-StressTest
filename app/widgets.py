"""The small widgets every tool panel is built out of.

Each of these takes theme colours from ``theme`` rather than being handed a
palette, so a colour is changed in one place and the whole program follows.
The point of the file is that a tool panel should be a list of fields, not a
hundred lines of grid arithmetic.
"""

import customtkinter as ctk

from app import theme


def section(parent, title):
    """A titled block with a rule under the heading.

    Returns the frame the caller should put rows into, not the outer frame --
    every caller wants the body, and handing back the wrapper just means
    every caller then reaches into it.
    """
    wrapper = ctk.CTkFrame(parent, corner_radius=5, fg_color=theme.SECTION_COLOR)
    wrapper.pack(fill="x", padx=4, pady=(0, 2))

    header = ctk.CTkLabel(
        wrapper,
        text=title,
        font=theme.HEADER_FONT,
        text_color=theme.TEXT_COLOR,
        anchor="w",
    )
    header.pack(fill="x", padx=7, pady=(3, 1))

    rule = ctk.CTkFrame(wrapper, height=1, fg_color=theme.RULE_COLOR)
    rule.pack(fill="x", padx=7, pady=(0, 3))

    body = ctk.CTkFrame(wrapper, fg_color="transparent")
    body.pack(fill="x", padx=7, pady=(0, 3))
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


def hint(parent, text, row, column=1, span=2, wrap=520):
    """Grey explanatory text under a field.

    Wrapped rather than truncated: these say why a setting exists, and half a
    sentence is worse than none.
    """
    widget = ctk.CTkLabel(
        parent,
        text=text,
        font=(theme.FONT_FAMILY, 9),
        text_color=theme.SUBTITLE_COLOR,
        anchor="w",
        justify="left",
        wraplength=wrap,
    )
    widget.grid(row=row, column=column, columnspan=span, sticky="w",
                pady=(0, 2))
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

        if field.kind == "multi":
            # A tick per choice, two to a row. y-cruncher's algorithms are
            # not alternatives -- any combination is a valid run -- so a
            # dropdown would be the wrong shape for them entirely.
            self.variables = {}
            holder = ctk.CTkFrame(parent, fg_color="transparent")
            holder.grid(row=row, column=1, columnspan=2, sticky="w",
                        padx=(8, 0), pady=1)
            chosen = set(str(field.default).replace(",", " ").split())
            for index, choice in enumerate(field.choices):
                variable = ctk.BooleanVar(value=choice in chosen)
                self.variables[choice] = variable
                ctk.CTkCheckBox(
                    holder,
                    text=choice,
                    variable=variable,
                    width=20,
                    checkbox_width=16,
                    checkbox_height=16,
                    font=theme.COMPACT_FONT,
                    text_color=theme.TEXT_COLOR,
                    fg_color=theme.TAB_SELECTED_COLOR,
                    hover_color=theme.TAB_HOVER_COLOR,
                    border_color=theme.RULE_COLOR,
                    command=self._changed,
                ).grid(row=index // 2, column=index % 2, sticky="w",
                       padx=(0, 18), pady=1)
            self.widget = holder
            self.variable = None
        elif field.kind == "bool":
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
                height=20,
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
        if self.field.kind == "multi":
            # In the order the tool lists them, not the order they were
            # ticked, so the same selection always produces the same
            # command line.
            return " ".join(name for name in self.field.choices
                            if self.variables[name].get())
        return self.field.coerce(self.variable.get())

    def set(self, value):
        if self.field.kind == "multi":
            chosen = set(str(value).replace(",", " ").split())
            for name, variable in self.variables.items():
                variable.set(name in chosen)
        elif self.field.kind == "bool":
            self.variable.set(bool(value))
        else:
            self.variable.set(str(value))

    def set_enabled(self, enabled):
        """Grey this row out, for a setting the tool is not free to choose.

        Prime95's own torture presets work out their FFT range and memory
        from the caches of the processor they find, so on one machine Large
        FFTs starts at 957K and on the next it does not. Those boxes are not
        ours to fill in under a preset like that: they are shown greyed, and
        what Prime95 decides is what runs. An editable box whose value is
        discarded is worse than no box at all.
        """
        state = "normal" if enabled else "disabled"
        if self.field.kind == "multi":
            for child in self.widget.winfo_children():
                try:
                    child.configure(state=state)
                except Exception:
                    pass
        else:
            try:
                self.widget.configure(state=state)
            except Exception:
                pass

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
        height=24,
        corner_radius=6,
        font=theme.COMPACT_BOLD,
        fg_color=colours[0],
        hover_color=colours[1],
        text_color=("#FFFFFF" if kind != "normal" else theme.TEXT_COLOR),
    )
