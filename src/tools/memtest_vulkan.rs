//! memtest_vulkan: the GPU's memory, tested the way TM5 tests the DIMMs.
//!
//! <https://github.com/GpuZelenograd/memtest_vulkan>
//!
//! The only tool here that tests video memory: an unstable memory overclock on
//! a graphics card shows up as artefacts, driver resets and crashes in games,
//! and none of the CPU-side tools would ever see it.
//!
//! It is a console program that writes both to the console and to
//! `memtest_vulkan.log` beside the executable, so it can be shown in its own
//! window and still be watched. It runs until it is stopped -- the "standard
//! 5-minute test" it announces is the length of the first pass, not the run --
//! so the time limit is the runner's.
//!
//! It re-executes itself, so the process that is started is a console that
//! spawns a worker. Killing it needs the whole tree, which is what the runner
//! does anyway.

use std::path::Path;

use crate::config;
use crate::core::errors;
use crate::core::toolbase::{
    cfg_bool, cfg_int, merged, path_str, patterns, unavailable, Config, Field, LaunchSpec, Preset,
    QuickStart, Tool, ToolError, CREATE_NEW_CONSOLE, CREATE_NO_WINDOW,
};

pub struct MemtestVulkan;

/// The three lengths a GPU memory test is actually run for: long enough to
/// catch a bad clock, long enough to trust, and left going.
pub const QUICK_LENGTHS: &[(&str, i64)] = &[("10 min", 10), ("30 min", 30), ("Infinite", 0)];

impl Tool for MemtestVulkan {
    fn key(&self) -> &'static str {
        "memtest_vulkan"
    }
    fn name(&self) -> &'static str {
        "memtest Vulkan"
    }
    fn blurb(&self) -> &'static str {
        "Video memory, tested through Vulkan. Nothing else here looks at the graphics card at \
         all, and a bad memory overclock on it shows up as artefacts and driver resets rather \
         than as anything the CPU tests would notice."
    }
    fn exe_globs(&self) -> &'static [&'static str] {
        &["memtest_vulkan*/memtest_vulkan*.exe", "memtest_vulkan*.exe"]
    }
    fn console(&self) -> bool {
        true
    }
    fn detection_note(&self) -> &'static str {
        "Output is read live. 'Error found' and 'Errors address range:' are its own failure \
         wording, taken from the binary; every good pass prints 'iteration. Passed'. A run that \
         ends without having been asked to stop is reported as stopped rather than passed."
    }
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::int("device", "GPU index", 1).range(1, 16).hint(
                "Which device to test, as memtest_vulkan numbers them in its own listing. 1 is \
                 the first.",
            ),
            Field::int("memory", "Memory to test", 0)
                .range(0, 1_048_576)
                .unit("MB")
                .hint(
                    "0 lets it size the test from the card's free memory, which is what you \
                     usually want.",
                ),
            Field::int("duration", "Stop after", 30)
                .range(0, 100_000)
                .unit("min")
                .hint(
                    "memtest_vulkan runs until it is stopped, so this limit is enforced here. 0 \
                     runs until you press Stop.",
                ),
            Field::boolean("show_window", "Show its window", true).hint(
                "Runs it in its own console. Failures are then read from memtest_vulkan.log, \
                 which carries the same text.",
            ),
        ]
    }
    fn quick_start(&self) -> QuickStart {
        QuickStart {
            preset: Some("Standard"),
            values: config! {"duration" => 15},
        }
    }
    fn presets(&self) -> Vec<Preset> {
        vec![
            Preset::new(
                "Standard",
                config! {"device" => 1, "memory" => 0},
                "The first GPU, with the test sized from its free memory.",
            ),
            Preset::new(
                "Second GPU",
                config! {"device" => 2, "memory" => 0},
                "For a machine with a second card, or an integrated one.",
            ),
            Preset::new("Custom", Config::new(), "Whatever is in the boxes below."),
        ]
    }

    /// The default preset at each of the three lengths.
    fn quick_actions(&self, root: &Path) -> Vec<(String, Config)> {
        let base = self.quick_config(root);
        QUICK_LENGTHS
            .iter()
            .map(|(label, minutes)| {
                (
                    label.to_string(),
                    merged(&base, &config! {"duration" => *minutes}),
                )
            })
            .collect()
    }

    fn quick_summary(&self, root: &Path) -> String {
        let lengths: Vec<&str> = QUICK_LENGTHS.iter().map(|(label, _)| *label).collect();
        format!(
            "{}  |  {}",
            self.quick_preset_name(root),
            lengths.join(" / ")
        )
    }

    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, ToolError> {
        let exe = self.locate(root).ok_or_else(|| {
            unavailable(
                "memtest_vulkan was not found. Put memtest_vulkan-vX.Y.Z.exe in a memtest_vulkan \
                 folder beside this program -- it is a single file, from \
                 github.com/GpuZelenograd/memtest_vulkan.",
            )
        })?;
        let folder = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        // It writes this beside itself, and appends across runs. Watched
        // rather than deleted, so the card's history is kept; the runner reads
        // only what is added after the run starts.
        let log = folder.join("memtest_vulkan.log");

        // The device index and the memory size as bare positional arguments,
        // in that order. With neither it prompts, which is fine for a person
        // and useless for a launcher.
        let device = cfg_int(config, "device", 1);
        let mut argv = vec![path_str(&exe), device.to_string()];
        let memory = cfg_int(config, "memory", 0);
        if memory > 0 {
            argv.push(memory.to_string());
        }

        let show = cfg_bool(config, "show_window", true);
        Ok(LaunchSpec {
            argv,
            cwd: folder,
            console: !show,
            watch_files: if show { vec![log] } else { Vec::new() },
            error_key: self.key().into(),
            summary: format!(
                "memtest_vulkan GPU {device}, {}",
                if memory != 0 {
                    format!("{memory} MB")
                } else {
                    "auto size".into()
                }
            ),
            duration_seconds: cfg_int(config, "duration", 0).max(0) as u64 * 60,
            // Without these, "no device selected" or an early exit during
            // init leaves a non-zero exit code and nothing to explain it, and
            // the graphics card is reported faulty when memtest_vulkan simply
            // never started.
            abort_patterns: patterns(errors::MEMTEST_VULKAN_ABORTED),
            creation_flags: if show {
                CREATE_NEW_CONSOLE
            } else {
                CREATE_NO_WINDOW
            },
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_config_is_standard_for_fifteen_minutes() {
        let root = Path::new("Z:\\nowhere");
        let config = MemtestVulkan.quick_config(root);
        assert_eq!(config["device"], 1);
        assert_eq!(config["memory"], 0);
        assert_eq!(config["duration"], 15);
        assert_eq!(
            MemtestVulkan.quick_summary(root),
            "Standard  |  10 min / 30 min / Infinite"
        );
        let actions = MemtestVulkan.quick_actions(root);
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[2].1["duration"], 0);
    }
}
