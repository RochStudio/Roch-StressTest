//! RAM Test Pro, opened at its own window.
//!
//! This is the one tool here with no way in. It has no command-line switches,
//! no settings file worth the name -- its settings.ini holds a window position
//! and a scale factor, nothing else -- and no registry keys. Thread count,
//! memory size, error limit and time limit exist only as four boxes in its
//! window, and they are set there.
//!
//! Its errors are still read. logs/log.txt carries a non-zero "Test errors
//! detected" and its own ERROR lines, and the one that reads like a failure
//! and is not -- "ERROR! Free up RAM..." -- is RAM Test Pro failing to get the
//! memory it was asked for, which is a setting to fix rather than a DIMM to
//! blame.

use std::fs;
use std::path::Path;

use crate::config;
use crate::core::toolbase::{
    cfg_int, limit_summary, path_str, remove_stale, unavailable, Config, Field, LaunchSpec,
    QuickStart, Tool, ToolError,
};

pub struct RamTestPro;

pub const WINDOW_TITLE: &str = "RAM Test Pro";

/// How long to wait for the window after starting the process. It is an
/// 83 MB .NET executable and takes a few seconds on a cold start.
pub const WINDOW_TIMEOUT_SECONDS: u64 = 40;

impl Tool for RamTestPro {
    fn key(&self) -> &'static str {
        "ramtest"
    }
    fn name(&self) -> &'static str {
        "RAM Test Pro"
    }
    fn blurb(&self) -> &'static str {
        "A commercial memory test with its own error counters and a stop-on-first-error \
         option. Set the profile, the size and the thread count in its own window."
    }
    fn exe_globs(&self) -> &'static [&'static str] {
        &["ram_test_pro*/**/RAM Test Pro.exe", "**/RAM Test Pro.exe"]
    }
    fn detection_note(&self) -> &'static str {
        "Failures are read from logs/log.txt: a non-zero 'Test errors detected' and its own \
         ERROR lines."
    }
    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0)
            .range(0, 100_000)
            .unit("min")
            .hint("0 lets the run you started in RAM Test Pro finish.")]
    }

    // Configured in its own window, so there is no tab here.
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
            unavailable("RAM Test Pro.exe was not found. Expected its folder beside this program.")
        })?;
        let folder = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        let log = folder.join("logs").join("log.txt");

        // The log is not committed -- it is a log -- and git does not keep
        // empty directories, so a fresh clone has no logs folder at all.
        if let Some(parent) = log.parent() {
            let _ = fs::create_dir_all(parent);
        }
        remove_stale(&log);

        Ok(LaunchSpec {
            argv: vec![path_str(&exe)],
            cwd: folder,
            console: false,
            watch_files: vec![log],
            error_key: self.key().into(),
            summary: "RAM Test Pro (settings entered in its own window)".into(),
            duration_seconds: cfg_int(config, "duration", 0).max(0) as u64 * 60,
            // No completion or abort pattern: "Test stopped by user" is what
            // it writes when a test is stopped from inside its own window,
            // which is now the middle of a run rather than the end of one.
            leave_open: true,
            ..Default::default()
        })
    }
}
