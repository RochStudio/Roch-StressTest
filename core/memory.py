"""Freeing memory back to the system, and watching how much there is.

A memory test is only testing memory it can actually get. Windows keeps a
standby list -- pages it has finished with but is holding in case they are
wanted again -- and counts them as "in use" for the purposes of an allocation.
Ask y-cruncher for 28 GB on a machine that has been up for a week and a good
part of that request goes to the page file, so the run measures an SSD rather
than the DIMMs.

Three operations free it, all through the same undocumented-but-stable call
that every memory cleaner uses, ``NtSetSystemInformation`` with
SystemMemoryListInformation:

    empty working sets            trims every process to its minimum
    purge the standby list        the big one, and the reason for this file
    purge the low-priority list   the cheap half of the same thing

They need SeProfileSingleProcessPrivilege, which an administrator has but
which is not enabled in the token by default; it has to be turned on first.
This is the same sequence as danskee's Memory Cleaner, which is where the
approach was checked against -- its DLL exports one CleanMemory and imports
exactly NtSetSystemInformation, AdjustTokenPrivileges and
LookupPrivilegeValue. Implemented here directly rather than bundled, so there
is no third-party binary in the loop and the privilege handling is visible.

Nothing here is destructive. Purging the standby list discards cached copies
of data that is already on disk; the pages are re-read if they are wanted
again. The cost is that the next access to a cached file is slower, which for
a machine about to be put under a stress test is not a cost at all.
"""

import ctypes
from ctypes import wintypes

from core import hardware

_NTDLL = ctypes.WinDLL("ntdll", use_last_error=True)
_ADVAPI32 = ctypes.WinDLL("advapi32", use_last_error=True)
_KERNEL32 = ctypes.WinDLL("kernel32", use_last_error=True)

# SYSTEM_INFORMATION_CLASS
SYSTEM_MEMORY_LIST_INFORMATION = 0x0050

# SYSTEM_MEMORY_LIST_COMMAND
MEMORY_EMPTY_WORKING_SETS = 2
MEMORY_FLUSH_MODIFIED_LIST = 3
MEMORY_PURGE_STANDBY_LIST = 4
MEMORY_PURGE_LOW_PRIORITY_STANDBY_LIST = 5

TOKEN_ADJUST_PRIVILEGES = 0x0020
TOKEN_QUERY = 0x0008
SE_PRIVILEGE_ENABLED = 0x00000002


class _LUID(ctypes.Structure):
    _fields_ = [("LowPart", wintypes.DWORD), ("HighPart", wintypes.LONG)]


class _LUID_AND_ATTRIBUTES(ctypes.Structure):
    _fields_ = [("Luid", _LUID), ("Attributes", wintypes.DWORD)]


class _TOKEN_PRIVILEGES(ctypes.Structure):
    _fields_ = [
        ("PrivilegeCount", wintypes.DWORD),
        ("Privileges", _LUID_AND_ATTRIBUTES * 1),
    ]


# Declared, not left to ctypes' defaults. Without these the process handle
# from GetCurrentProcess is marshalled as a 32-bit int, OpenProcessToken gets
# a truncated handle, and the whole sequence fails with no error worth the
# name -- it simply reports the privilege was not enabled. Every call below
# was checked against a real elevated run before this file was trusted.
_KERNEL32.GetCurrentProcess.restype = wintypes.HANDLE
_KERNEL32.CloseHandle.argtypes = [wintypes.HANDLE]
_ADVAPI32.OpenProcessToken.argtypes = [
    wintypes.HANDLE, wintypes.DWORD, ctypes.POINTER(wintypes.HANDLE),
]
_ADVAPI32.OpenProcessToken.restype = wintypes.BOOL
_ADVAPI32.LookupPrivilegeValueW.argtypes = [
    wintypes.LPCWSTR, wintypes.LPCWSTR, ctypes.POINTER(_LUID),
]
_ADVAPI32.LookupPrivilegeValueW.restype = wintypes.BOOL
_ADVAPI32.AdjustTokenPrivileges.argtypes = [
    wintypes.HANDLE, wintypes.BOOL, ctypes.POINTER(_TOKEN_PRIVILEGES),
    wintypes.DWORD, ctypes.c_void_p, ctypes.c_void_p,
]
_ADVAPI32.AdjustTokenPrivileges.restype = wintypes.BOOL
_NTDLL.NtSetSystemInformation.argtypes = [
    ctypes.c_int, ctypes.c_void_p, ctypes.c_ulong,
]
_NTDLL.NtSetSystemInformation.restype = ctypes.c_long


def enable_privilege(name):
    """Turn on one privilege in this process's token. True when it took.

    Being an administrator is not the same as having the privilege enabled:
    it is present but disabled until asked for, and NtSetSystemInformation
    fails with a plain access-denied if it is not.
    """
    token = wintypes.HANDLE()
    if not _ADVAPI32.OpenProcessToken(
        _KERNEL32.GetCurrentProcess(),
        TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
        ctypes.byref(token),
    ):
        return False
    try:
        luid = _LUID()
        if not _ADVAPI32.LookupPrivilegeValueW(None, name, ctypes.byref(luid)):
            return False
        privileges = _TOKEN_PRIVILEGES()
        privileges.PrivilegeCount = 1
        privileges.Privileges[0].Luid = luid
        privileges.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED
        if not _ADVAPI32.AdjustTokenPrivileges(
            token, False, ctypes.byref(privileges),
            ctypes.sizeof(privileges), None, None
        ):
            return False
        # AdjustTokenPrivileges reports success even when it changed nothing,
        # so the real answer is in the last error.
        return ctypes.get_last_error() == 0
    finally:
        _KERNEL32.CloseHandle(token)


def _set_memory_list(command):
    """Issue one SystemMemoryListInformation command. True on success."""
    value = ctypes.c_int(command)
    status = _NTDLL.NtSetSystemInformation(
        SYSTEM_MEMORY_LIST_INFORMATION,
        ctypes.byref(value),
        ctypes.sizeof(value),
    )
    return status == 0


# What each step is called in the report, in the order they are run. Working
# sets first: trimming processes moves their pages onto the standby list,
# where the next step can then free them. The other order frees less.
STEPS = (
    ("working sets", MEMORY_EMPTY_WORKING_SETS),
    ("standby list", MEMORY_PURGE_STANDBY_LIST),
    ("low-priority standby list", MEMORY_PURGE_LOW_PRIORITY_STANDBY_LIST),
)


class Result:
    """What a clean actually achieved."""

    def __init__(self, before_mb, after_mb, done, failed):
        self.before_mb = before_mb
        self.after_mb = after_mb
        self.done = done
        self.failed = failed

    @property
    def freed_mb(self):
        return max(0, self.after_mb - self.before_mb)

    def describe(self):
        text = "Memory cleaned: {:,} MB free before, {:,} MB after".format(
            self.before_mb, self.after_mb
        )
        if self.freed_mb:
            text += " ({:,} MB freed)".format(self.freed_mb)
        else:
            text += " (nothing to free)"
        if self.failed:
            text += ". Could not clear: " + ", ".join(self.failed)
        return text


def clean():
    """Empty working sets and purge the standby lists.

    Returns a Result, including anything that could not be done, rather than
    raising: failing to free memory is a disappointment, not a reason to stop
    a stress test from starting.
    """
    before = hardware.available_ram_mb()
    enable_privilege("SeProfileSingleProcessPrivilege")
    enable_privilege("SeIncreaseQuotaPrivilege")

    done, failed = [], []
    for label, command in STEPS:
        if _set_memory_list(command):
            done.append(label)
        else:
            failed.append(label)
    return Result(before, hardware.available_ram_mb(), done, failed)


def reading():
    """(available MB, total MB, percent used) for the live display."""
    total = hardware.total_ram_mb()
    available = hardware.available_ram_mb()
    used = 0 if not total else int(round((total - available) * 100.0 / total))
    return available, total, used
