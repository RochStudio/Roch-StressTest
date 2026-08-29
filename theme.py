"""The Roch look, in one place.

Every colour is a ``(light, dark)`` pair, which is what CustomTkinter wants:
hand it the tuple once and it re-paints the widget itself when the appearance
mode changes, so nothing here has to be re-applied on a theme switch.

The palette is the one Roch Viewer uses, kept identical on purpose -- the two
programs sit side by side on the same desktop and reading one after the other
should not feel like changing tools. Roch GPU OC's black/grey/red carries the
same idea into WPF: dark surfaces, grey chrome, red reserved for the thing you
actually need to look at. Here that red is a failure, which is the one event
this program exists to make unmissable.
"""

# Surfaces, lightest content plane first. The steps between the dark values
# are small because a dark theme reads as flat when they are not: 161616 to
# 202020 is the whole range the panels have to work in.
BG_COLOR = ("#F1F5F9", "#161616")
BG_COLOR2 = ("#FFFFFF", "#1C1C1C")
SECTION_COLOR = ("#E2E8F0", "#1C1C1C")
ROW_COLOR = ("#F8FAFC", "#202020")
BORDER_COLOR = ("#CBD5E1", "#101010")
HIGHLIGHT_COLOR = ("#E8EEF5", "#1D1D1D")
HEADER_COLOR = ("#E2E8F0", "#222222")

# Type.
TEXT_COLOR = ("#0F172A", "#FFFFFF")
SUBTITLE_COLOR = ("#475569", "#B0B0B0")
VALUE_COLOR = ("#B91C1C", "#FF4D4D")
RULE_COLOR = ("#94A3B8", "#4A4A4A")

# Tabs and buttons.
TAB_SELECTED_COLOR = ("#2563EB", "#1A3C5D")
TAB_UNSELECTED_COLOR = ("#D7E1EC", "#2E2E2E")
TAB_HOVER_COLOR = ("#3B82F6", "#2A5579")
TAB_UNSELECTED_HOVER_COLOR = ("#C5D2E0", "#3A3A3A")

# Run states. Idle is deliberately quiet, and the three outcomes are the only
# saturated colours in the program -- a glance at the status strip from across
# the room is the whole point of them.
IDLE_COLOR = SUBTITLE_COLOR
RUNNING_COLOR = ("#2563EB", "#6AA9FF")
PASS_COLOR = ("#15803D", "#4ADE80")
FAIL_COLOR = ("#B91C1C", "#FF4D4D")
WARN_COLOR = ("#B45309", "#FBBF24")

# The Start button is the one control that should read as a button rather than
# as chrome, so it gets the accent rather than the unselected-tab grey.
START_COLOR = ("#15803D", "#1F5F38")
START_HOVER_COLOR = ("#166534", "#2A7A48")
STOP_COLOR = ("#B91C1C", "#5D1A1A")
STOP_HOVER_COLOR = ("#991B1B", "#7A2626")

# Consolas throughout, for the same reason Roch Viewer uses it: these panels
# are full of numbers that want to line up in a column.
FONT_FAMILY = "Consolas"
FONT_SIZE = 11

GLOBAL_FONT = (FONT_FAMILY, FONT_SIZE)
COMPACT_FONT = (FONT_FAMILY, FONT_SIZE)
COMPACT_BOLD = (FONT_FAMILY, FONT_SIZE, "bold")
HEADER_FONT = (FONT_FAMILY, FONT_SIZE, "bold")
TAB_FONT = (FONT_FAMILY, 11, "bold")
TITLE_FONT = (FONT_FAMILY, 13, "bold")
LOG_FONT = (FONT_FAMILY, 10)
STATUS_FONT = (FONT_FAMILY, 11, "bold")

# Row metrics, matched to Roch Viewer so a row is the same height in both.
ROW_HEIGHT = 18
ROW_PADX = 4
ROW_PADY = 0
SECTION_GAP = 3
LABEL_MINSIZE = 150
FIELD_WIDTH = 130
