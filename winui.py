"""Reading and filling in another program's window.

Used for exactly one tool. RAM Test Pro has no command line, no settings file
worth the name -- its settings.ini holds a window position and nothing else --
and no registry keys. Its thread count, memory size, error limit and time
limit exist only as boxes in its window, so the only way to set them from here
is to type into them.

That is a brittle thing to do, and it is done defensively:

  * Controls are found by their *label*, not by position or index. The label
    "Max Errors" is part of what the program means; the fact that its box is
    the third one down is not.
  * Every value written is read back. A box that did not take the value is
    reported rather than assumed, so a run never starts with settings that
    are not the ones on screen.

If a future version renames a label or drops a box, this fails loudly at the
point of setting up, which is the only acceptable way for it to fail. A stress
test that quietly ran with the wrong memory size is worse than one that did
not start.
"""

import ctypes
from ctypes import wintypes

_USER32 = ctypes.WinDLL("user32", use_last_error=True)

MF_BYPOSITION = 0x0400
WM_COMMAND = 0x0111
BM_GETCHECK = 0x00F0
GWL_STYLE = -16
BS_TYPEMASK = 0x0000000F
BS_RADIOBUTTON = 0x00000004
BS_AUTORADIOBUTTON = 0x00000009
WM_SETTEXT = 0x000C
WM_GETTEXT = 0x000D
WM_GETTEXTLENGTH = 0x000E
BM_CLICK = 0x00F5

_ENUM_PROC = ctypes.WINFUNCTYPE(
    wintypes.BOOL, wintypes.HWND, wintypes.LPARAM
)

# Declared rather than left to ctypes' defaults. The lparam of WM_GETTEXT and
# WM_SETTEXT is a pointer, and on 64-bit that does not fit the signed int
# ctypes assumes -- it raises "int too long to convert" the moment a buffer
# lands above 2GB, which is most of the time.
_USER32.SendMessageTimeoutW.argtypes = [
    wintypes.HWND, wintypes.UINT, wintypes.WPARAM, ctypes.c_void_p,
    wintypes.UINT, wintypes.UINT, ctypes.POINTER(ctypes.c_size_t),
]
_USER32.SendMessageTimeoutW.restype = ctypes.c_size_t
_USER32.GetWindowThreadProcessId.argtypes = [
    wintypes.HWND, ctypes.POINTER(wintypes.DWORD),
]
_USER32.GetWindowThreadProcessId.restype = wintypes.DWORD
_USER32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
_USER32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
_USER32.IsWindowVisible.argtypes = [wintypes.HWND]
_USER32.EnumWindows.argtypes = [_ENUM_PROC, wintypes.LPARAM]
_USER32.EnumChildWindows.argtypes = [wintypes.HWND, _ENUM_PROC, wintypes.LPARAM]
_USER32.GetMenu.argtypes = [wintypes.HWND]
_USER32.GetMenu.restype = wintypes.HMENU
_USER32.GetSubMenu.argtypes = [wintypes.HMENU, ctypes.c_int]
_USER32.GetSubMenu.restype = wintypes.HMENU
_USER32.GetMenuItemCount.argtypes = [wintypes.HMENU]
_USER32.GetMenuItemID.argtypes = [wintypes.HMENU, ctypes.c_int]
_USER32.GetMenuStringW.argtypes = [
    wintypes.HMENU, wintypes.UINT, wintypes.LPWSTR, ctypes.c_int, wintypes.UINT
]
_USER32.PostMessageW.argtypes = [
    wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM
]
_USER32.GetWindowLongW.argtypes = [wintypes.HWND, ctypes.c_int]
_USER32.GetWindowLongW.restype = wintypes.LONG


class Control:
    """One child window: what it is, what it says, and where it sits."""

    def __init__(self, hwnd, class_name, text, rect):
        self.hwnd = hwnd
        self.class_name = class_name
        self.text = text
        self.left, self.top, self.right, self.bottom = rect

    @property
    def middle(self):
        return (self.top + self.bottom) / 2.0

    def is_edit(self):
        # WinForms controls are named "WindowsForms10.Edit.app.0...."; a
        # plain Win32 dialog just says "Edit". Both turn up here, because
        # RAM Test Pro's own window is WinForms and the message boxes it
        # puts up are ordinary #32770 dialogs.
        return self.class_name == "Edit" or ".Edit." in self.class_name

    def is_button(self):
        return self.class_name == "Button" or ".Button." in self.class_name

    def __repr__(self):
        # WinForms names a control "WindowsForms10.Button.app.0...", and the
        # interesting part is the second piece. A plain Win32 dialog just
        # says "Button", with no pieces at all -- so taking [1] blindly threw
        # IndexError on every control of every standard dialog, which is most
        # of them once a tool other than RAM Test Pro is driven.
        pieces = self.class_name.split(".")
        kind = pieces[1] if len(pieces) > 1 else self.class_name
        return "Control(%r, %r)" % (kind, self.text)


# Every read of another program's window text goes through a timeout. A plain
# SendMessage blocks until the target's message loop answers, and a program
# sitting on a modal dialog -- or simply hung -- never does. Enumerating the
# desktop that way once cost a run fifty seconds of nothing before it was
# noticed; with a timeout the worst case is a window that reports no text.
SMTO_ABORTIFHUNG = 0x0002
_MESSAGE_TIMEOUT_MS = 1000


def _send(hwnd, message, wparam, lparam):
    result = ctypes.c_size_t(0)
    ok = _USER32.SendMessageTimeoutW(
        hwnd, message, wparam, lparam, SMTO_ABORTIFHUNG,
        _MESSAGE_TIMEOUT_MS, ctypes.byref(result),
    )
    return result.value if ok else 0


def _text_of(hwnd):
    length = _send(hwnd, WM_GETTEXTLENGTH, 0, 0)
    if not length:
        return ""
    buffer = ctypes.create_unicode_buffer(length + 1)
    _send(hwnd, WM_GETTEXT, length + 1, buffer)
    return buffer.value


def _pid_of(hwnd):
    owner = wintypes.DWORD()
    _USER32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
    return owner.value


def windows_of(pid):
    """Visible top-level windows belonging to *pid*.

    Filtered by owner before anything is read, so no message is ever sent to
    another program's window. That is what keeps a hung window elsewhere on
    the desktop from becoming this program's problem.
    """
    found = []

    @_ENUM_PROC
    def visit(hwnd, _lparam):
        if _pid_of(hwnd) == pid and _USER32.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True

    _USER32.EnumWindows(visit, 0)
    return found


def find_window(title, pid=None):
    """The visible top-level window with this title, or None.

    Given a process id, only that process's windows are looked at.
    """
    candidates = windows_of(pid) if pid else []
    if pid is None:
        @_ENUM_PROC
        def visit(hwnd, _lparam):
            if _USER32.IsWindowVisible(hwnd):
                candidates.append(hwnd)
            return True

        _USER32.EnumWindows(visit, 0)
    for hwnd in candidates:
        if _text_of(hwnd) == title:
            return hwnd
    return None


def message_box(pid, ignore=()):
    """A visible message box belonging to *pid*, as (hwnd, text, ok button).

    Returns None when there is none. This is how a refused setting is
    noticed: RAM Test Pro answers a memory size it cannot use with a modal
    box -- "Memory block size must be at least 50 MB." -- and simply does not
    start. Without looking for it, the run reports nothing wrong and nothing
    happens, which is the worst of both.

    Filtered by process id because #32770 is the standard dialog class and
    every other program on the desktop uses it too.
    """
    found = []

    for hwnd in windows_of(pid):
        if hwnd in ignore:
            continue
        name = ctypes.create_unicode_buffer(256)
        _USER32.GetClassNameW(hwnd, name, 256)
        if name.value == "#32770":
            found.append(hwnd)
            break
    if not found:
        return None
    hwnd = found[0]
    children = controls(hwnd)
    message = " ".join(
        c.text.strip() for c in children
        if c.class_name == "Static" and c.text.strip()
    )
    ok = next((c for c in children if c.is_button()
               and c.text.strip() in ("OK", "Yes", "Close")), None)
    return hwnd, message, ok


def controls(parent):
    """Every child control of *parent*, at any depth."""
    collected = []

    @_ENUM_PROC
    def visit(hwnd, _lparam):
        name = ctypes.create_unicode_buffer(256)
        _USER32.GetClassNameW(hwnd, name, 256)
        rect = wintypes.RECT()
        _USER32.GetWindowRect(hwnd, ctypes.byref(rect))
        collected.append(Control(
            hwnd, name.value, _text_of(hwnd),
            (rect.left, rect.top, rect.right, rect.bottom),
        ))
        return True

    _USER32.EnumChildWindows(parent, visit, 0)
    return collected


def box_beside(found, label, tolerance=14):
    """The text box on the same line as the label reading *label*.

    Matched by vertical centre rather than by order, because "the box next to
    Max Errors" survives a layout change and "the third box down" does not.
    The box must also start to the right of the label, which is what stops a
    left-hand column matching a right-hand one on the same line.

    Ampersands are ignored on both sides. A plain Win32 dialog carries the
    keyboard accelerator in the caption -- Prime95's is really "Number of
    cores to &torture test:" -- and matching on the label as drawn is the
    only way to write it down the way it appears on screen.
    """
    def plain(text):
        return text.replace("&", "").strip()

    labels = [c for c in found if plain(c.text) == plain(label)]
    if not labels:
        return None
    anchor = labels[0]
    candidates = [
        c for c in found
        if c.is_edit()
        and c.left >= anchor.left
        and abs(c.middle - anchor.middle) <= tolerance
    ]
    if not candidates:
        return None
    return min(candidates, key=lambda c: abs(c.middle - anchor.middle))


def button(found, label):
    for control in found:
        if control.is_button() and control.text.strip() == label:
            return control
    return None


def menu_command(hwnd, wanted):
    """Post the menu command whose caption contains *wanted*. True when sent.

    Prime95's torture dialog is the one thing about that program which cannot
    be set from a file. It does not read prime.txt when it opens -- it comes
    up on Blend with figures of its own no matter what the file holds -- so
    the only way to put it into a known state is to open it from the menu and
    press the button, which is what this exists for.
    """
    menu = _USER32.GetMenu(hwnd)
    if not menu:
        return False
    for index in range(_USER32.GetMenuItemCount(menu)):
        sub = _USER32.GetSubMenu(menu, index)
        if not sub:
            continue
        for item in range(_USER32.GetMenuItemCount(sub)):
            caption = ctypes.create_unicode_buffer(256)
            _USER32.GetMenuStringW(sub, item, caption, 256, MF_BYPOSITION)
            if wanted.lower() in caption.value.lower():
                _USER32.PostMessageW(
                    hwnd, WM_COMMAND, _USER32.GetMenuItemID(sub, item), 0)
                return True
    return False


def is_radio(control):
    style = _USER32.GetWindowLongW(control.hwnd, GWL_STYLE)
    return (style & BS_TYPEMASK) in (BS_RADIOBUTTON, BS_AUTORADIOBUTTON)


def checked(control):
    """True when a radio button or check box is ticked."""
    return _send(control.hwnd, BM_GETCHECK, 0, 0) == 1


def radio(found, label):
    """The radio button whose caption starts with *label*.

    Captions carry the ampersand of their keyboard accelerator, so the button
    drawn as "Large FFTs (stresses memory controller and RAM)" is really
    "&Large FFTs (...)". Stripping it is what makes the visible name findable.
    """
    for control in found:
        if control.is_button() and is_radio(control)                 and control.text.replace("&", "").startswith(label):
            return control
    return None


def labelled(found, wanted):
    """The first button whose caption contains *wanted*, ampersand ignored."""
    for control in found:
        if control.is_button()                 and wanted.lower() in control.text.replace("&", "").lower():
            return control
    return None


def set_text(control, value):
    """Type a value into a box and read it back. True when it took."""
    buffer = ctypes.create_unicode_buffer(str(value))
    _send(control.hwnd, WM_SETTEXT, 0, buffer)
    return _text_of(control.hwnd).strip() == str(value).strip()


def read_text(control):
    return _text_of(control.hwnd).strip()


def click(control):
    _send(control.hwnd, BM_CLICK, 0, 0)
