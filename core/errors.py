"""Deciding, from a tool's output, whether the machine just failed.

A stress test is only useful if somebody notices the failure. Half of these
tools announce a problem in a window nobody is watching at 3am, and one of
them (Linpack) does not announce it at all -- it prints a table and the
failure is a word in the eighth column. So every tool's output is scanned
here, by the same code, against patterns that mean "this run is over".

Two rules kept the pattern lists honest:

  * Match what the tool prints when it *fails*, not every line containing the
    word "error". Prime95's banner mentions error checking; y-cruncher prints
    an error-tolerance figure at startup. Both would fire a naive \berror\b.
  * Never match on a count of zero. "0 errors" is the line a passing TM5 run
    writes, and it contains the word.
"""

import re

# Patterns common to every console tool: a Windows-level crash of the child
# process shows up as text before the process dies.
_COMMON = [
    r"\bSTATUS_ACCESS_VIOLATION\b",
    r"\bunhandled exception\b",
]

# Prime95 writes these to results.txt. "Torture Test completed" with a
# non-zero error count is the summary line; the others appear the moment a
# worker finds a bad result.
PRIME95 = [
    r"FATAL ERROR",
    r"[Hh]ardware failure",
    r"possible hardware failure",
    r"ILLEGAL SUMOUT",
    r"SUMOUT MISMATCH",
    r"ROUND OFF",
    r"Torture Test completed.*?[1-9]\d*\s+errors",
    r"self-test.*?fail",
]

# y-cruncher's stress tester stops on the first bad comparison and says so.
# "Coefficient is too large" is the one that catches an arithmetic slip that
# has not yet corrupted a checksum.
YCRUNCHER = [
    # y-cruncher prints "Running VSTv3: Passed" once per iteration, so the
    # negative of that line is the signal that matters most.
    r"Running\s+\S+\s*:\s*(?!Passed)(?:FAIL|Failed|ERROR)",
    r"\bFAILED\b",
    r"Redundancy Check Failed",
    r"Coefficient is too large",
    r"\bhardware (?:error|failure)\b",
    r"An error (?:has )?occurred",
    r"Validation Fail",
    # Anchored, because y-cruncher's startup menu prints the row
    # "6   Stop on Error:      Enabled" and an unanchored "ERROR:" fails
    # every run before the first test has even started.
    r"^\s*ERROR\s*:",
    r"Computation Error",
    r"does not match",
]

# TestMem5 0.13.1 writes Log.txt beside TM5.exe, so its failures are readable
# after all. These are its own message templates, lifted from the strings in
# TM5.exe and TM5.dll rather than guessed:
#
#   "Error in test #%li through %s."
#   "The testing is completed, is revealed %li of errors!"
#   "detected %li error(s)."
#   "Critical error, programm stopped!"   (its spelling, not a typo here)
#
# and the ones that must NOT fire, which are the same shape:
#
#   "Testing completed in %s, no errors."
#   "The testing is completed, of errors is not detected."
#   "Testing stopped by user"
#
# The last group is the reason this list is separate from the generic memory
# patterns. TM5 distinguishes a failure of the memory under test from a
# failure of TM5 itself -- "Failed to allocate memory for testing", "WARNING!
# Failed to lock memory", a missing MT0.DLL -- and says so in as many words:
# "This is not a failure of tested memory." Reporting those as instability
# would send somebody chasing a timing that was never at fault.
TESTMEM5 = [
    r"Error in test",
    r"is revealed [1-9]\d* of errors",
    r"detected [1-9]\d* error",
    r"(?:Critical|Fatal) error, programm stopped",
    r"encountered an error, type",
    r"caused by a failure of tested memory",
]

# RAM Test Pro writes logs/log.txt beside its executable. Its own wording:
#
#   "Test errors detected: 0"     the verdict at the end of every run
#   "Test stopped by user at ..." how a run that was cut short ends
#   "Current Cycle 1"             the start of each cycle
#
# and the one that must NOT count:
#
#   "ERROR! Free up RAM or reduce memory size for test!"
#
# That last one is RAM Test Pro failing to get the memory it was asked for,
# which is a setting to fix, not a DIMM to blame. Same distinction TM5 makes,
# and the same reason for keeping this list separate from the generic one.
RAMTEST = [
    r"Test errors detected:\s*[1-9]\d*",
    r"^\s*ERROR\s*=",
    r"^\s*ERROR\s+\d",
]

RAMTEST_ABORTED = [
    r"Test stopped by user",
]

# Generic memory-test wording, kept for anything not covered above.
MEMTEST = [
    # "4 errors" fails; "0 errors" -- the line a clean cycle writes every
    # pass -- cannot match, because the count may not start with a zero.
    r"\b[1-9]\d*\s+errors?\b",
    r"\berrors?\b\s*[:=]\s*[1-9]\d*",
    r"\bERROR\b[^\n]{0,40}\baddress\b",
    r"memory error",
    r"\bBAD\b[^\n]{0,20}\bmemory\b",
]

# Linpack is handled structurally by scan_linpack_row rather than by regex --
# the failure is a column, not a phrase -- but a hard stop still prints.
LINPACK = [
    r"\bFAIL\b",
    r"severe instability",
    r"RESIDUAL MISMATCH",
]


# TM5 finishes its cycles and then just sits there with its window open, so
# "done" has to be read out of the log rather than waited for as a process
# exit. These are its two endings, and they are not the same thing: completing
# the configured cycles is a pass, and being stopped early -- by a person, by
# Windows, by another program taking the memory -- is not.
TESTMEM5_COMPLETE = [
    r"Testing completed",
    r"The testing is completed, of errors is not detected",
]

TESTMEM5_ABORTED = [
    r"Testing stopped by",
]


# y-cruncher's last line when a stress run ends of its own accord. Without it
# a run that ended early -- its window closed, its console interrupted --
# exits cleanly and is indistinguishable from one that ran to the end.
YCRUNCHER_COMPLETE = [
    r"Test Finished",
]


# memtest_vulkan's own wording, out of the binary. Every good pass prints
# "iteration. Passed"; a bad one prints "Error found. Mode ..., total errors"
# and then the address range. Its "Standard 5-minute test" banner is not a
# result and must not be mistaken for one.
# Cinebench renders and scores; it has no notion of a wrong answer to report.
# What it does have is the ways it can fail to run at all, and a driver that
# falls over mid-render is exactly the instability worth catching.
CINEBENCH = [
    r"Error loading",
    r"could not be (?:created|initialized)",
    # Written with two literal backspace bytes where word boundaries
    # were meant, so the compiled pattern held two 0x08 bytes instead
    # and could never match a line Cinebench prints. Its neighbours
    # carry no boundaries either, so plain text is what was intended.
    r"rendering failed",
    r"out of memory",
]

# 3DMark's command-line runner reports a failed workload rather than a score.
THREEDMARK = [
    r"^Error:",
    r"workload .*failed",
    r"device (?:removed|hung|reset)",
    r"DXGI_ERROR",
]

MEMTEST_VULKAN = [
    r"Error found",
    r"Errors address range",
    r"error found",
    r"^\s*FAILED",
]

# It stops only when told to, so any ending it announces is the end of the
# run rather than a verdict; the runner's clock is what finishes a pass.
MEMTEST_VULKAN_ABORTED = [
    r"Test cancelled, no device selected",
    r"early exit during init",
]


# Lines that look like a failure and are not one. Checked first, so a tool
# that complains about something harmless on its way up cannot be mistaken
# for a machine that is wrong.
#
# Cinebench is the one that needs this. R24 and R26 are Cinema 4D 2024/2026,
# which register their modules at startup and narrate every one they cannot
# find -- "[reflection registration] could not be initialized because
# net.maxon.mvp.widgetclasses.intslider is missing" and a dozen like it. They
# are warnings, they are printed before anything has been rendered, and the
# application then runs perfectly well. Opening R24 reported FAILED on the
# strength of one of them.
#
# The rule is narrow on purpose: a line Cinebench itself labels WARNING is
# not a failure. Anything it labels an error still is.
NOT_FAILURES = {
    "cinebench": [
        r"^WARNING:",
    ],
}

_EXCUSED = {key: [re.compile(p, re.IGNORECASE) for p in patterns]
            for key, patterns in NOT_FAILURES.items()}


def _compile(patterns):
    return [re.compile(p, re.IGNORECASE) for p in patterns + _COMMON]


def compile_plain(patterns):
    """Compile a list without adding the common crash patterns."""
    return [re.compile(p, re.IGNORECASE) for p in patterns]


def matches(text, compiled):
    """The first line of *text* matching any of *compiled*, or None."""
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        for pattern in compiled:
            if pattern.search(stripped):
                return stripped
    return None


PATTERNS = {
    "prime95": _compile(PRIME95),
    "ycruncher": _compile(YCRUNCHER),
    "testmem5": _compile(TESTMEM5 + MEMTEST),
    "ramtest": _compile(RAMTEST),
    "linpack": _compile(LINPACK),
    "memtest_vulkan": _compile(MEMTEST_VULKAN),
    "cinebench": _compile(CINEBENCH),
    "3dmark11": _compile(THREEDMARK),
}


def scan(text, tool_key):
    """Return the first line of *text* that means this run has failed.

    None when the text is clean. Only the first hit is returned: once a run
    has failed there is nothing further to learn from it, and the caller is
    about to stop the process anyway.
    """
    compiled = PATTERNS.get(tool_key, _compile([]))
    excused = _EXCUSED.get(tool_key, ())
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        if any(pattern.search(stripped) for pattern in excused):
            continue
        for pattern in compiled:
            if pattern.search(stripped):
                return stripped
    return None


# A Linpack result row: size, LDA, alignment, seconds, GFlops, residual,
# normalised residual, and the check. Anything else on stdout is a banner.
_LINPACK_ROW = re.compile(
    r"^\s*(\d+)\s+(\d+)\s+(\d+)\s+([\d.]+)\s+([\d.]+)\s+"
    r"([\d.eE+-]+)\s+([\d.eE+-]+)\s+(\w+)\s*$"
)


def scan_linpack_row(line):
    """Parse one Linpack output row.

    Returns ``(gflops, residual, passed)`` for a result row and None for
    anything else. The check column is the honest failure signal: Linpack
    keeps running after a bad solve and simply writes something other than
    "pass" in that column, so a run watched only for crashes looks clean.
    """
    match = _LINPACK_ROW.match(line)
    if not match:
        return None
    try:
        gflops = float(match.group(5))
    except ValueError:
        return None
    residual = match.group(6)
    passed = match.group(8).strip().lower() == "pass"
    return gflops, residual, passed
