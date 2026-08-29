"""What the machine is, in the few numbers a stress test needs to size itself.

Every default in this program is derived from these: how many workers Prime95
should start, how much memory y-cruncher may take, how big a Linpack problem
fits. Reading them wrong is worse than not reading them -- a Linpack size
chosen from an over-reported free-memory figure swaps rather than stresses --
so each one falls back to a conservative answer rather than a guess.

No third-party dependency: everything here is ctypes against kernel32 plus one
registry read.
"""

import ctypes
import os
from ctypes import wintypes

_KERNEL32 = ctypes.WinDLL("kernel32", use_last_error=True)

# SYSTEM_LOGICAL_PROCESSOR_INFORMATION.Relationship
_RELATION_PROCESSOR_CORE = 0


class _MEMORYSTATUSEX(ctypes.Structure):
    _fields_ = [
        ("dwLength", wintypes.DWORD),
        ("dwMemoryLoad", wintypes.DWORD),
        ("ullTotalPhys", ctypes.c_ulonglong),
        ("ullAvailPhys", ctypes.c_ulonglong),
        ("ullTotalPageFile", ctypes.c_ulonglong),
        ("ullAvailPageFile", ctypes.c_ulonglong),
        ("ullTotalVirtual", ctypes.c_ulonglong),
        ("ullAvailVirtual", ctypes.c_ulonglong),
        ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
    ]


def _memory_status():
    status = _MEMORYSTATUSEX()
    status.dwLength = ctypes.sizeof(_MEMORYSTATUSEX)
    if not _KERNEL32.GlobalMemoryStatusEx(ctypes.byref(status)):
        return None
    return status


def total_ram_mb():
    """Installed memory in MB, or 8192 when it cannot be read."""
    status = _memory_status()
    if status is None:
        return 8192
    return int(status.ullTotalPhys // (1024 * 1024))


def available_ram_mb():
    """Free memory in MB right now, or half of installed when unreadable."""
    status = _memory_status()
    if status is None:
        return total_ram_mb() // 2
    return int(status.ullAvailPhys // (1024 * 1024))


def logical_cores():
    """Logical processors, which is what a worker count is counted in."""
    return os.cpu_count() or 1


class _PROCESSOR_INFO(ctypes.Structure):
    # SYSTEM_LOGICAL_PROCESSOR_INFORMATION. The union at the end is 16 bytes
    # on x64 and is never read here, so it is reserved rather than described.
    _fields_ = [
        ("ProcessorMask", ctypes.c_size_t),
        ("Relationship", ctypes.c_int),
        ("Reserved", ctypes.c_ubyte * 16),
    ]


def physical_cores():
    """Physical cores, counted from the processor-relationship table.

    Falls back to half the logical count -- the answer for every SMT machine
    and wrong only in the direction of asking for fewer workers -- when the
    table cannot be read. GetLogicalProcessorInformation describes only the
    first processor group, so on a machine with more than 64 logical
    processors this undercounts; the Ex form that does not is a great deal of
    structure-walking for a number used to seed a default the user can edit.
    """
    try:
        length = wintypes.DWORD(0)
        _KERNEL32.GetLogicalProcessorInformation(None, ctypes.byref(length))
        if not length.value:
            raise OSError("empty processor information")
        count = length.value // ctypes.sizeof(_PROCESSOR_INFO)
        buffer = (_PROCESSOR_INFO * count)()
        if not _KERNEL32.GetLogicalProcessorInformation(
            ctypes.byref(buffer), ctypes.byref(length)
        ):
            raise OSError(ctypes.get_last_error())
        cores = sum(
            1 for entry in buffer if entry.Relationship == _RELATION_PROCESSOR_CORE
        )
        if cores:
            return cores
    except Exception:
        pass
    return max(1, logical_cores() // 2)


def cpu_vendor():
    """"AuthenticAMD", "GenuineIntel", or "" when the key is unreadable.

    Decides which of the two Linpack binaries is the right one: the AMD build
    forces AVX2 paths that the Intel build gates behind a vendor check, and
    running the Intel build on Ryzen is the classic way to get a Linpack score
    that means nothing.
    """
    try:
        import winreg

        with winreg.OpenKey(
            winreg.HKEY_LOCAL_MACHINE,
            r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
        ) as key:
            return str(winreg.QueryValueEx(key, "VendorIdentifier")[0]).strip()
    except Exception:
        return ""


def cpu_name():
    """The marketing string, for the run header in the log."""
    try:
        import winreg

        with winreg.OpenKey(
            winreg.HKEY_LOCAL_MACHINE,
            r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
        ) as key:
            return str(winreg.QueryValueEx(key, "ProcessorNameString")[0]).strip()
    except Exception:
        return "Unknown CPU"


def is_amd():
    return cpu_vendor().upper() == "AUTHENTICAMD"


def describe():
    """One line naming the machine, written at the top of every run log."""
    return (
        f"{cpu_name()} -- {physical_cores()}C/{logical_cores()}T, "
        f"{total_ram_mb() / 1024:.1f} GB RAM"
    )


def describe_cpu():
    """The same line without the memory size.

    For the toolbar, which shows memory live a few pixels to the right and
    does not need to say 31.3 GB twice.
    """
    return f"{cpu_name()} -- {physical_cores()}C/{logical_cores()}T"
