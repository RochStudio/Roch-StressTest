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

# TM5 and RAM Test Pro are windowed and only write a log when they are told
# to, so these fire on whatever they do leave behind. The count guard matters
# most here: both print a running error total every cycle.
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


def _compile(patterns):
    return [re.compile(p, re.IGNORECASE) for p in patterns + _COMMON]


PATTERNS = {
    "prime95": _compile(PRIME95),
    "ycruncher": _compile(YCRUNCHER),
    "testmem5": _compile(MEMTEST),
    "ramtest": _compile(MEMTEST),
    "linpack": _compile(LINPACK),
}


def scan(text, tool_key):
    """Return the first line of *text* that means this run has failed.

    None when the text is clean. Only the first hit is returned: once a run
    has failed there is nothing further to learn from it, and the caller is
    about to stop the process anyway.
    """
    compiled = PATTERNS.get(tool_key, _compile([]))
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
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
