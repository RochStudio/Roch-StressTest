//! Deciding, from a tool's output, whether the machine just failed.
//!
//! A stress test is only useful if somebody notices the failure. Half of these
//! tools announce a problem in a window nobody is watching at 3am, and one of
//! them (Linpack) does not announce it at all -- it prints a table and the
//! failure is a word in the eighth column. So every tool's output is scanned
//! here, by the same code, against patterns that mean "this run is over".
//!
//! Two rules kept the pattern lists honest:
//!
//!   * Match what the tool prints when it *fails*, not every line containing
//!     the word "error". Prime95's banner mentions error checking; y-cruncher
//!     prints an error-tolerance figure at startup. Both would fire a naive
//!     `\berror\b`.
//!   * Never match on a count of zero. "0 errors" is the line a passing TM5
//!     run writes, and it contains the word.
//!
//! The patterns are the Python ones, character for character, compiled with
//! `fancy-regex` because one of them (y-cruncher's) needs a lookahead.
//! Every pattern is case-insensitive and searched (not anchored) against a
//! stripped line, as `re.search(..., re.IGNORECASE)` did.

use std::collections::HashMap;
use std::sync::OnceLock;

use fancy_regex::Regex;

use crate::core::pyutil::{splitlines, strip};

/// Patterns common to every console tool: a Windows-level crash of the child
/// process shows up as text before the process dies.
pub const COMMON: &[&str] = &[r"\bSTATUS_ACCESS_VIOLATION\b", r"\bunhandled exception\b"];

/// Prime95 writes these to results.txt. "Torture Test completed" with a
/// non-zero error count is the summary line; the others appear the moment a
/// worker finds a bad result.
pub const PRIME95: &[&str] = &[
    r"FATAL ERROR",
    r"[Hh]ardware failure",
    r"possible hardware failure",
    r"ILLEGAL SUMOUT",
    r"SUMOUT MISMATCH",
    r"ROUND OFF",
    r"Torture Test completed.*?[1-9]\d*\s+errors",
    r"self-test.*?fail",
];

/// y-cruncher's stress tester stops on the first bad comparison and says so.
/// "Coefficient is too large" is the one that catches an arithmetic slip that
/// has not yet corrupted a checksum.
pub const YCRUNCHER: &[&str] = &[
    // y-cruncher prints "Running VSTv3: Passed" once per iteration, so the
    // negative of that line is the signal that matters most.
    r"Running\s+\S+\s*:\s*(?!Passed)(?:FAIL|Failed|ERROR)",
    r"\bFAILED\b",
    r"Redundancy Check Failed",
    r"Coefficient is too large",
    r"\bhardware (?:error|failure)\b",
    r"An error (?:has )?occurred",
    r"Validation Fail",
    // Anchored, because y-cruncher's startup menu prints the row
    // "6   Stop on Error:      Enabled" and an unanchored "ERROR:" fails
    // every run before the first test has even started.
    r"^\s*ERROR\s*:",
    r"Computation Error",
    r"does not match",
];

/// TestMem5 0.13.1 writes Log.txt beside TM5.exe. These are its own message
/// templates, lifted from the strings in TM5.exe and TM5.dll rather than
/// guessed:
///
/// ```text
///   "Error in test #%li through %s."
///   "The testing is completed, is revealed %li of errors!"
///   "detected %li error(s)."
///   "Critical error, programm stopped!"   (its spelling, not a typo here)
/// ```
///
/// and the ones that must NOT fire, which are the same shape: "Testing
/// completed in %s, no errors.", "The testing is completed, of errors is not
/// detected.", "Testing stopped by user". TM5 distinguishes a failure of the
/// memory under test from a failure of TM5 itself -- "Failed to allocate
/// memory for testing", "WARNING! Failed to lock memory", a missing MT0.DLL --
/// and says so: "This is not a failure of tested memory." Reporting those as
/// instability would send somebody chasing a timing that was never at fault.
pub const TESTMEM5: &[&str] = &[
    r"Error in test",
    r"is revealed [1-9]\d* of errors",
    r"detected [1-9]\d* error",
    r"(?:Critical|Fatal) error, programm stopped",
    r"encountered an error, type",
    r"caused by a failure of tested memory",
];

/// RAM Test Pro writes logs/log.txt beside its executable. "Test errors
/// detected: 0" is the verdict at the end of every run; "ERROR! Free up RAM or
/// reduce memory size for test!" must NOT count -- that is RAM Test Pro
/// failing to get the memory it was asked for, a setting to fix rather than a
/// DIMM to blame.
pub const RAMTEST: &[&str] = &[
    r"Test errors detected:\s*[1-9]\d*",
    r"^\s*ERROR\s*=",
    r"^\s*ERROR\s+\d",
];

pub const RAMTEST_ABORTED: &[&str] = &[r"Test stopped by user"];

/// Generic memory-test wording, kept for anything not covered above.
pub const MEMTEST: &[&str] = &[
    // "4 errors" fails; "0 errors" -- the line a clean cycle writes every
    // pass -- cannot match, because the count may not start with a zero.
    r"\b[1-9]\d*\s+errors?\b",
    r"\berrors?\b\s*[:=]\s*[1-9]\d*",
    r"\bERROR\b[^\n]{0,40}\baddress\b",
    r"memory error",
    r"\bBAD\b[^\n]{0,20}\bmemory\b",
];

/// Linpack is handled structurally by `scan_linpack_row` rather than by regex
/// -- the failure is a column, not a phrase -- but a hard stop still prints.
/// These are also exactly what Linpack Extended's own driver prints when it
/// stops a run: "FAIL - severe instability detected" and "RESIDUAL MISMATCH -
/// instability detected".
pub const LINPACK: &[&str] = &[r"\bFAIL\b", r"severe instability", r"RESIDUAL MISMATCH"];

/// TM5 finishes its cycles and then just sits there with its window open, so
/// "done" has to be read out of the log rather than waited for as a process
/// exit. Completing the configured cycles is a pass; being stopped early is
/// not.
pub const TESTMEM5_COMPLETE: &[&str] = &[
    r"Testing completed",
    r"The testing is completed, of errors is not detected",
];

pub const TESTMEM5_ABORTED: &[&str] = &[r"Testing stopped by"];

/// y-cruncher's last line when a stress run ends of its own accord. Without it
/// a run that ended early -- its window closed, its console interrupted --
/// exits cleanly and is indistinguishable from one that ran to the end.
pub const YCRUNCHER_COMPLETE: &[&str] = &[r"Test Finished"];

/// Cinebench renders and scores; it has no notion of a wrong answer to report.
/// What it does have is the ways it can fail to run at all, and a driver that
/// falls over mid-render is exactly the instability worth catching.
pub const CINEBENCH: &[&str] = &[
    r"Error loading",
    r"could not be (?:created|initialized)",
    r"rendering failed",
    r"out of memory",
];

/// 3DMark's command-line runner reports a failed workload rather than a score.
pub const THREEDMARK: &[&str] = &[
    r"^Error:",
    r"workload .*failed",
    r"device (?:removed|hung|reset)",
    r"DXGI_ERROR",
];

/// memtest_vulkan's own wording, out of the binary. Every good pass prints
/// "iteration. Passed"; a bad one prints "Error found. Mode ..., total
/// errors" and then the address range.
pub const MEMTEST_VULKAN: &[&str] = &[
    r"Error found",
    r"Errors address range",
    r"error found",
    r"^\s*FAILED",
];

/// It stops only when told to, so any ending it announces is the end of the
/// run rather than a verdict; the runner's clock is what finishes a pass.
pub const MEMTEST_VULKAN_ABORTED: &[&str] = &[
    r"Test cancelled, no device selected",
    r"early exit during init",
];

/// Lines that look like a failure and are not one. Checked first, so a tool
/// that complains about something harmless on its way up cannot be mistaken
/// for a machine that is wrong.
///
/// Cinebench R24 and R26 register their modules at startup and narrate every
/// one they cannot find -- "[reflection registration] could not be
/// initialized because net.maxon.mvp.widgetclasses.intslider is missing".
/// They are warnings, and the rule is narrow on purpose: a line Cinebench
/// itself labels WARNING is not a failure.
pub fn not_failures(tool_key: &str) -> &'static [&'static str] {
    match tool_key {
        "cinebench" => &[r"^WARNING:"],
        _ => &[],
    }
}

fn compile_one(pattern: &str) -> Regex {
    Regex::new(&format!("(?i){pattern}"))
        .unwrap_or_else(|error| panic!("bad failure pattern {pattern:?}: {error}"))
}

/// Compile a list without adding the common crash patterns.
pub fn compile_plain<S: AsRef<str>>(patterns: &[S]) -> Vec<Regex> {
    patterns.iter().map(|p| compile_one(p.as_ref())).collect()
}

fn compile_with_common(patterns: &[&[&str]]) -> Vec<Regex> {
    patterns
        .iter()
        .flat_map(|list| list.iter())
        .chain(COMMON.iter())
        .map(|p| compile_one(p))
        .collect()
}

fn search(pattern: &Regex, text: &str) -> bool {
    pattern.is_match(text).unwrap_or(false)
}

/// The first line of *text* matching any of *compiled*, stripped, or None.
pub fn matches<'a>(text: &'a str, compiled: &[Regex]) -> Option<&'a str> {
    for line in splitlines(text) {
        let stripped = strip(line);
        if stripped.is_empty() {
            continue;
        }
        if compiled.iter().any(|pattern| search(pattern, stripped)) {
            return Some(stripped);
        }
    }
    None
}

struct Compiled {
    patterns: HashMap<&'static str, Vec<Regex>>,
    fallback: Vec<Regex>,
    excused: HashMap<&'static str, Vec<Regex>>,
}

fn compiled() -> &'static Compiled {
    static CELL: OnceLock<Compiled> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut patterns = HashMap::new();
        patterns.insert("prime95", compile_with_common(&[PRIME95]));
        patterns.insert("ycruncher", compile_with_common(&[YCRUNCHER]));
        patterns.insert("testmem5", compile_with_common(&[TESTMEM5, MEMTEST]));
        patterns.insert("ramtest", compile_with_common(&[RAMTEST]));
        patterns.insert("linpack", compile_with_common(&[LINPACK]));
        // Linpack Extended's driver prints the LINPACK phrases itself when it
        // stops a run. Its key differs from "linpack", and in the Python
        // version that left it with only the common patterns, so a driver
        // that printed "FAIL - severe instability detected" and exited 0 was
        // reported as stopped rather than failed -- the opposite of what the
        // documentation promises. Keyed here explicitly.
        patterns.insert("linpack_extended", compile_with_common(&[LINPACK]));
        patterns.insert("memtest_vulkan", compile_with_common(&[MEMTEST_VULKAN]));
        patterns.insert("cinebench", compile_with_common(&[CINEBENCH]));
        patterns.insert("3dmark11", compile_with_common(&[THREEDMARK]));
        let mut excused = HashMap::new();
        excused.insert("cinebench", compile_plain(not_failures("cinebench")));
        Compiled {
            patterns,
            fallback: compile_with_common(&[]),
            excused,
        }
    })
}

/// Return the first line of *text* that means this run has failed.
///
/// None when the text is clean. Only the first hit is returned: once a run
/// has failed there is nothing further to learn from it, and the caller is
/// about to stop the process anyway.
pub fn scan(text: &str, tool_key: &str) -> Option<String> {
    let all = compiled();
    let patterns = all.patterns.get(tool_key).unwrap_or(&all.fallback);
    let excused: &[Regex] = all
        .excused
        .get(tool_key)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    for line in splitlines(text) {
        let stripped = strip(line);
        if stripped.is_empty() {
            continue;
        }
        if excused.iter().any(|pattern| search(pattern, stripped)) {
            continue;
        }
        if patterns.iter().any(|pattern| search(pattern, stripped)) {
            return Some(stripped.to_string());
        }
    }
    None
}

fn linpack_row() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        // size, LDA, alignment, seconds, GFlops, residual, normalised
        // residual, and the check. Anything else on stdout is a banner.
        Regex::new(
            r"^\s*(\d+)\s+(\d+)\s+(\d+)\s+([\d.]+)\s+([\d.]+)\s+([\d.eE+-]+)\s+([\d.eE+-]+)\s+(\w+)\s*$",
        )
        .expect("linpack row pattern")
    })
}

/// One parsed Linpack result row.
#[derive(Debug, Clone, PartialEq)]
pub struct LinpackRow {
    pub gflops: f64,
    pub residual: String,
    pub passed: bool,
}

/// Parse one Linpack output row.
///
/// Returns the GFlops, residual and verdict for a result row and None for
/// anything else. The check column is the honest failure signal: Linpack
/// keeps running after a bad solve and simply writes something other than
/// "pass" in that column, so a run watched only for crashes looks clean.
pub fn scan_linpack_row(line: &str) -> Option<LinpackRow> {
    let captures = linpack_row().captures(line).ok()??;
    let gflops: f64 = captures.get(5)?.as_str().parse().ok()?;
    let residual = captures.get(6)?.as_str().to_string();
    let passed = strip(captures.get(8)?.as_str()).to_lowercase() == "pass";
    Some(LinpackRow {
        gflops,
        residual,
        passed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_compiles() {
        let lists: &[&[&str]] = &[
            COMMON,
            PRIME95,
            YCRUNCHER,
            TESTMEM5,
            RAMTEST,
            RAMTEST_ABORTED,
            MEMTEST,
            LINPACK,
            TESTMEM5_COMPLETE,
            TESTMEM5_ABORTED,
            YCRUNCHER_COMPLETE,
            CINEBENCH,
            THREEDMARK,
            MEMTEST_VULKAN,
            MEMTEST_VULKAN_ABORTED,
        ];
        for list in lists {
            assert_eq!(compile_plain(list).len(), list.len());
        }
        let _ = compiled();
    }

    #[test]
    fn prime95_failures() {
        assert!(scan(
            "FATAL ERROR: Rounding was 0.4990028871, expected less than 0.4",
            "prime95"
        )
        .is_some());
        assert!(scan(
            "Possible hardware failure, consult readme.txt file.",
            "prime95"
        )
        .is_some());
        assert!(scan(
            "Torture Test completed 12 tests in 2 hours - 3 errors, 0 warnings.",
            "prime95"
        )
        .is_some());
        assert!(scan(
            "Torture Test completed 12 tests in 2 hours - 0 errors, 0 warnings.",
            "prime95"
        )
        .is_none());
        assert!(scan("Self-test 1024K passed!", "prime95").is_none());
        assert!(scan("SELF-TEST 1024K FAILED", "prime95").is_some());
    }

    #[test]
    fn ycruncher_failures_and_passes() {
        assert!(scan("Running VSTv3: Passed  Test Speed: 10.4", "ycruncher").is_none());
        assert!(scan("Running VSTv3: Failed", "ycruncher").is_some());
        assert!(scan("Running FFTv4:  ERROR", "ycruncher").is_some());
        assert!(scan("6   Stop on Error:      Enabled", "ycruncher").is_none());
        assert!(scan("ERROR: something", "ycruncher").is_some());
        assert!(scan("   error : something", "ycruncher").is_some());
        assert!(scan("Coefficient is too large.", "ycruncher").is_some());
        assert!(scan(
            "Redundancy Check Failed: Coefficient is too large.",
            "ycruncher"
        )
        .is_some());
        assert!(scan("Test Finished", "ycruncher").is_none());
    }

    #[test]
    fn testmem5_wording() {
        assert!(scan("Error in test #3 through 0:01:23.", "testmem5").is_some());
        assert!(scan(
            "The testing is completed, is revealed 4 of errors!",
            "testmem5"
        )
        .is_some());
        assert!(scan("Testing completed in 0:30:00, no errors.", "testmem5").is_none());
        assert!(scan(
            "The testing is completed, of errors is not detected.",
            "testmem5"
        )
        .is_none());
        assert!(scan("Testing stopped by user", "testmem5").is_none());
        assert!(scan("Critical error, programm stopped!", "testmem5").is_some());
        assert!(scan("detected 0 error(s).", "testmem5").is_none());
        assert!(scan("detected 2 error(s).", "testmem5").is_some());
        assert!(scan("Failed to allocate memory for testing", "testmem5").is_none());
        assert!(scan(
            "WARNING! Failed to lock memory. This is not a failure of tested memory.",
            "testmem5"
        )
        .is_none());
        assert!(scan("Errors: 0", "testmem5").is_none());
        assert!(scan("Errors: 7", "testmem5").is_some());
    }

    #[test]
    fn ramtest_wording() {
        assert!(scan("Test errors detected: 0", "ramtest").is_none());
        assert!(scan("Test errors detected: 12", "ramtest").is_some());
        assert!(scan(
            "ERROR! Free up RAM or reduce memory size for test!",
            "ramtest"
        )
        .is_none());
        assert!(scan("ERROR = 0x1234 at 0xdeadbeef", "ramtest").is_some());
        assert!(scan("ERROR 3 at address", "ramtest").is_some());
    }

    #[test]
    fn linpack_extended_driver_failures_are_failures() {
        assert!(scan("FAIL - severe instability detected", "linpack_extended").is_some());
        assert!(scan(
            "RESIDUAL MISMATCH - instability detected (residual checks can be turned off in the config)",
            "linpack_extended"
        )
        .is_some());
        assert!(scan("Residual checks: OFF", "linpack_extended").is_none());
        assert!(scan("All tests successfully passed", "linpack_extended").is_none());
    }

    #[test]
    fn memtest_vulkan_and_others() {
        assert!(scan(
            "  981 iteration. Passed 30.0198 seconds  written: 1.0GB",
            "memtest_vulkan"
        )
        .is_none());
        assert!(scan(
            "Error found. Mode NEXT_RE_READ, total errors 0x20 out of 0x2C000000 (0.00000007%)",
            "memtest_vulkan"
        )
        .is_some());
        assert!(scan("Standard 5-minute test of 1: Bus=0x01", "memtest_vulkan").is_none());
        assert!(scan("Error: workload failed", "3dmark11").is_some());
        assert!(scan("DXGI_ERROR_DEVICE_REMOVED", "3dmark11").is_some());
    }

    #[test]
    fn cinebench_warnings_are_excused() {
        let line = "WARNING: [reflection registration] could not be initialized because net.maxon.mvp.widgetclasses.intslider is missing";
        assert!(scan(line, "cinebench").is_none());
        assert!(scan(
            "[reflection registration] could not be initialized",
            "cinebench"
        )
        .is_some());
        assert!(scan("Rendering failed", "cinebench").is_some());
    }

    #[test]
    fn common_patterns_apply_to_every_tool() {
        assert!(scan("STATUS_ACCESS_VIOLATION", "occt").is_some());
        assert!(scan("Unhandled exception at 0x0", "prime95").is_some());
        assert!(scan("nothing to see", "occt").is_none());
    }

    #[test]
    fn scan_returns_the_first_failing_line_stripped() {
        let text = "ok\n   Running SFTv4: Passed\n  FATAL ERROR: x  \nROUND OFF\n";
        assert_eq!(scan(text, "prime95").as_deref(), Some("FATAL ERROR: x"));
    }

    #[test]
    fn matches_uses_plain_patterns() {
        let finished = compile_plain(TESTMEM5_COMPLETE);
        assert_eq!(
            matches("x\nTesting completed in 1:00, no errors.\n", &finished),
            Some("Testing completed in 1:00, no errors.")
        );
        assert_eq!(matches("STATUS_ACCESS_VIOLATION", &finished), None);
    }

    #[test]
    fn linpack_rows() {
        let row = scan_linpack_row(
            "38736  38736  4      195.381  198.6743 1.068245e-09 2.511566e-02   pass",
        )
        .unwrap();
        assert_eq!(row.residual, "1.068245e-09");
        assert!(row.passed);
        assert!((row.gflops - 198.6743).abs() < 1e-9);
        let bad = scan_linpack_row("1000 1000 4 0.01 50.2 1.2e-12 3.4e-02 fail").unwrap();
        assert!(!bad.passed);
        assert!(scan_linpack_row(
            "Size   LDA    Align. Time(s)    GFlops   Residual     Residual(norm) Check"
        )
        .is_none());
        assert!(scan_linpack_row("Performance Summary (GFlops)").is_none());
        assert!(scan_linpack_row("1000 1000 4 0.01 1.2.3 1e-1 1e-1 pass").is_none());
    }
}
