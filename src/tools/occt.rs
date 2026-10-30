//! OCCT, opened at its own window.
//!
//! OCCT is the one tool here that covers the whole machine from a single
//! program: an AVX2 CPU test, a memory test, a combined CPU+RAM run, a 3D
//! graphics load and a power-supply test that drives CPU and GPU together.
//! Each of them checks its own results, which is the same class of tool as
//! Prime95 and Linpack rather than a benchmark.
//!
//! It is opened rather than driven. Its command line exists, but the settings
//! that matter are chosen very differently per test type, and the free edition
//! limits an unattended run to an hour anyway. Nothing is watched from here:
//! OCCT keeps its own logs and shows errors on screen as they happen; what the
//! runner still does is hold the run to a limit if one was set and notice if
//! it dies.

use std::path::Path;

use crate::config;
use crate::core::toolbase::{
    cfg_int, limit_summary, path_str, unavailable, Config, Field, LaunchSpec, QuickStart, Tool,
    ToolError,
};

pub struct Occt;

impl Tool for Occt {
    fn key(&self) -> &'static str {
        "occt"
    }
    fn name(&self) -> &'static str {
        "OCCT"
    }
    fn blurb(&self) -> &'static str {
        "CPU, memory, combined and power tests in one program, each checking its own answers. \
         The AVX2 CPU test and the memory test are the two worth reaching for after a clock or \
         timing change; the power test loads CPU and GPU together and is the hardest thing here \
         on a PSU."
    }
    // A single portable executable, either beside this program or installed.
    fn exe_globs(&self) -> &'static [&'static str] {
        &["OCCT*/OCCT*.exe", "OCCT*.exe"]
    }
    fn external_globs(&self) -> Vec<String> {
        vec![
            r"C:\Program Files\OCCT\OCCT*.exe".into(),
            r"C:\Program Files (x86)\OCCT\OCCT*.exe".into(),
        ]
    }
    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0)
            .range(0, 100_000)
            .unit("min")
            .hint("0 lets the test you set up in OCCT run to its own end.")]
    }
    // Configured in OCCT's own window, so there is no tab here.
    fn has_tab(&self) -> bool {
        false
    }
    fn quick_start(&self) -> QuickStart {
        QuickStart {
            preset: None,
            values: config! {"duration" => 0},
        }
    }
    fn quick_actions(&self, root: &Path) -> Vec<(String, Config)> {
        vec![("Open".into(), self.quick_config(root))]
    }
    fn quick_summary(&self, root: &Path) -> String {
        limit_summary(self, root)
    }
    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, ToolError> {
        let exe = self.locate(root).ok_or_else(|| {
            unavailable(
                "OCCT.exe was not found. It is a single portable executable -- put it in an OCCT \
                 folder beside this program.",
            )
        })?;
        Ok(LaunchSpec {
            argv: vec![path_str(&exe)],
            cwd: exe.parent().map(Path::to_path_buf).unwrap_or_default(),
            console: false,
            error_key: self.key().into(),
            summary: "OCCT (test chosen in its own window)".into(),
            duration_seconds: cfg_int(config, "duration", 0).max(0) as u64 * 60,
            leave_open: true,
            ..Default::default()
        })
    }
}
