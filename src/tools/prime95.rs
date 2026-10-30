//! Prime95's torture test, opened at its own dialog.
//!
//! Prime95 has exactly two command-line switches worth anything here: `-t` to
//! start the torture test and `-W` to point it at a working directory.
//!
//! Most references, including Prime95's own undoc.txt, put the torture keys in
//! `local.txt`. In 30.19 that is a migration path rather than where they live:
//! give it a local.txt and it folds those keys into prime.txt, deletes
//! local.txt, and carries on. So prime.txt is written directly.
//!
//! Pointing `-W` at our own scratch directory rather than the distribution
//! folder matters more than it looks. Prime95 writes prime.txt, results.txt
//! and worker save files wherever it is told to work; sending them to
//! LOCALAPPDATA leaves the unpacked p95 folder byte-identical to what was
//! downloaded, and gives results.txt a stable path to watch for failures.

use std::path::Path;

use crate::config;
use crate::core::settings;
use crate::core::toolbase::{
    cfg_int, path_str, remove_stale, unavailable, write_crlf, Config, Field, LaunchSpec,
    QuickStart, Tool, ToolError,
};

pub struct Prime95;

/// The four keys written to prime.txt, and no torture settings at all.
/// StressTester=1 is the "just stress testing" answer that stops the GIMPS
/// welcome dialog waiting for somebody, and it is also what makes Prime95
/// raise its torture dialog on startup -- which is the point here. The other
/// two mark the file as already migrated, suppressing the version prompt.
pub const PRIME_TXT: &str =
    "StressTester=1\nUsePrimenet=0\nV24OptionsConverted=1\nWGUID_version=2\n";

impl Tool for Prime95 {
    fn key(&self) -> &'static str {
        "prime95"
    }
    fn name(&self) -> &'static str {
        "Prime95"
    }
    fn blurb(&self) -> &'static str {
        "FFT torture test. Smallest and Small FFTs are the heat and power test; Large, Blend \
         and Huge lean on the memory controller and RAM."
    }
    fn exe_globs(&self) -> &'static [&'static str] {
        &["p95*/prime95.exe", "prime95*/prime95.exe", "prime95.exe"]
    }
    fn detection_note(&self) -> &'static str {
        "Failures are read from results.txt, which Prime95 writes on every error, so a worker \
         that fails while you are away is still caught."
    }
    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0)
            .range(0, 100_000)
            .unit("min")
            .hint("0 lets whatever you chose in Prime95's dialog run until you press Stop.")]
    }

    // Configured in Prime95's own window, so there is no tab here. Its
    // torture dialog cannot be preset from a file -- it opens on Blend
    // whatever prime.txt holds, and resets to Blend every time it is opened,
    // even mid-run -- so a tab of FFT sizes and memory figures could never be
    // shown to agree with what Prime95 was actually going to do.
    fn has_tab(&self) -> bool {
        false
    }

    fn quick_start(&self) -> QuickStart {
        QuickStart {
            preset: None,
            values: config! {"duration" => 0},
        }
    }

    /// One button, and it opens the tool rather than starting a run.
    fn quick_actions(&self, root: &Path) -> Vec<(String, Config)> {
        vec![("Open".into(), self.quick_config(root))]
    }

    /// The card has no preset to name, so it says what will happen.
    fn quick_summary(&self, root: &Path) -> String {
        let limit = cfg_int(&self.quick_config(root), "duration", 0);
        let mut text = "Opens its torture dialog".to_string();
        if limit != 0 {
            text += &format!("  |  {limit} min");
        }
        text
    }

    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, ToolError> {
        let exe = self.locate(root).ok_or_else(|| {
            unavailable(
                "prime95.exe was not found. Expected a folder like p95v3019b20.win64 beside this \
                 program.",
            )
        })?;

        let work = settings::run_dir("prime95");
        write_crlf(&work.join("prime.txt"), PRIME_TXT)?;

        // A local.txt left over from an earlier version of this program would
        // be migrated on top of what was just written.
        remove_stale(&work.join("local.txt"));

        // Started fresh each run so the failure watcher never reports an
        // error left over from the last one.
        let results = work.join("results.txt");
        remove_stale(&results);

        // No -t: that would start a torture test from the file before there
        // was a chance to choose one.
        Ok(LaunchSpec {
            argv: vec![path_str(&exe), format!("-W{}", path_str(&work))],
            cwd: exe.parent().map(Path::to_path_buf).unwrap_or_default(),
            console: false,
            watch_files: vec![results],
            error_key: self.key().into(),
            summary: "Prime95 (test chosen in its own dialog)".into(),
            duration_seconds: cfg_int(config, "duration", 0).max(0) as u64 * 60,
            ..Default::default()
        })
    }
}
