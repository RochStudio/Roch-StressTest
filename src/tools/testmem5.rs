//! TestMem5, opened at its own window.
//!
//! Everything that makes one TM5 profile different from another -- which
//! tests run, in what order, for how many cycles -- lives in a .cfg in its
//! bin folder, and picking one *is* the configuration. That is why the
//! profile is chosen in TM5's own window rather than here.
//!
//! It was chosen here once, and the cycle count with it. TM5 keeps that count
//! in the file rather than on the command line, so overriding it meant writing
//! a copy of the profile -- which TM5 then displayed as its configuration,
//! "Roch active" instead of the name that was picked, and a run given two
//! cycles was seen going into a third.
//!
//! What is kept is the part worth having. TM5 0.13.1 appends to Log.txt beside
//! TM5.exe -- the configuration it loaded, the memory it took, every error,
//! and how the run ended -- so a failure at 3am is caught however the run was
//! set up.

use std::fs;
use std::path::Path;

use crate::config;
use crate::core::errors;
use crate::core::toolbase::{
    cfg_int, limit_summary, path_str, patterns, remove_stale, unavailable, Config, Field,
    LaunchSpec, QuickStart, Tool, ToolError,
};

pub struct TestMem5;

/// The one file name an earlier version wrote into TM5's bin folder, used
/// only when a cycle count overrode the profile's own.
pub const WORKING_NAME: &str = "Roch active";
pub const WORKING_CFG: &str = "Roch active.cfg";

/// What TM5 opens on when nothing has been picked yet. TM5 remembers its
/// Testing settings in TM5.ini -- the profile as "Config File" (Universal 2
/// when missing) and "Test at least, minutes" as "Minutes", where its own
/// infinity button writes 0 (90 when missing). The defaults are set by seeding
/// the keys that are absent, never by overwriting a choice made in TM5's own
/// window.
pub const DEFAULT_PROFILE: &str = "1usmus v3 @ 1usmus.cfg";
pub const DEFAULT_SETTINGS: &[(&str, &str)] = &[("Config File", DEFAULT_PROFILE), ("Minutes", "0")];

/// Add each of DEFAULT_SETTINGS that TM5.ini does not have yet.
pub fn seed_defaults(folder: &Path) {
    let ini = folder.join("TM5.ini");
    let mut lines: Vec<String> = match fs::read(&ini) {
        // Read byte for byte (as Latin-1) and written back the same way, so
        // whatever TM5 put in the file survives untouched apart from the
        // lines added.
        Ok(bytes) => {
            let text: String = bytes.iter().map(|&b| b as char).collect();
            crate::core::pyutil::splitlines(&text)
                .into_iter()
                .map(str::to_string)
                .collect()
        }
        Err(_) => Vec::new(),
    };
    let present: Vec<String> = lines
        .iter()
        .filter_map(|line| {
            line.split_once('=')
                .map(|(key, _)| key.trim().to_lowercase())
        })
        .collect();
    let mut missing: Vec<(&str, &str)> = DEFAULT_SETTINGS
        .iter()
        .filter(|(key, _)| !present.contains(&key.to_lowercase()))
        .copied()
        .collect();
    if !folder.join("bin").join(DEFAULT_PROFILE).exists() {
        missing.retain(|(key, _)| *key != "Config File");
    }
    if missing.is_empty() {
        return;
    }
    if !lines
        .iter()
        .any(|line| line.trim().to_lowercase() == "[settings]")
    {
        lines.insert(0, "[Settings]".to_string());
    }
    let index = lines
        .iter()
        .position(|line| line.trim().to_lowercase() == "[settings]")
        .unwrap_or(0);
    for (offset, (key, value)) in missing.iter().enumerate() {
        lines.insert(index + 1 + offset, format!("{key}={value}"));
    }
    let text = lines.join("\r\n") + "\r\n";
    let bytes: Vec<u8> = text
        .chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect();
    let _ = fs::write(&ini, bytes);
}

impl Tool for TestMem5 {
    fn key(&self) -> &'static str {
        "testmem5"
    }
    fn name(&self) -> &'static str {
        "TestMem5"
    }
    fn blurb(&self) -> &'static str {
        "The memory test overclockers actually use. The anta777 profiles are the ones worth \
         running: Extreme for a first pass, Absolut when you want to be sure. Pick the profile \
         and the cycles in TM5's own window."
    }
    fn exe_globs(&self) -> &'static [&'static str] {
        &["TestMem5*/TM5.exe", "TM5.exe"]
    }
    fn detection_note(&self) -> &'static str {
        "TM5 0.13.1 appends to Log.txt beside TM5.exe, and that is read live: 'Error in test \
         #N', a non-zero error total, and a crash blamed on the tested memory all stop the run."
    }
    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0)
            .range(0, 100_000)
            .unit("min")
            .hint("0 lets the cycles you set in TM5 run to their end.")]
    }

    // Configured in TM5's own window, so there is no tab here.
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
            unavailable("TM5.exe was not found. Expected a TestMem5 folder beside this program.")
        })?;
        let folder = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        let log = folder.join("Log.txt");

        // Started fresh so the watcher never reports an error left from the
        // last run. TM5 appends, so without this every run inherits the one
        // before it.
        remove_stale(&log);

        seed_defaults(&folder);

        // No arguments at all. Given a Config File it starts that profile
        // immediately; given none it opens and waits, which is the point.
        Ok(LaunchSpec {
            argv: vec![path_str(&exe)],
            cwd: folder,
            console: false,
            watch_files: vec![log],
            error_key: self.key().into(),
            summary: "TestMem5 (profile and cycles set in its own window)".into(),
            duration_seconds: cfg_int(config, "duration", 0).max(0) as u64 * 60,
            completion_patterns: patterns(errors::TESTMEM5_COMPLETE),
            // No abort pattern. "Testing stopped by user" is what it writes
            // when a test is stopped from inside its own window -- to open
            // Settings, to change a profile. Now that the window is yours,
            // that is the middle of a run, not the end of one.
            leave_open: true,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("roch-tm5-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).unwrap();
        dir
    }

    #[test]
    fn seeds_a_missing_ini() {
        let dir = temp("fresh");
        fs::write(dir.join("bin").join(DEFAULT_PROFILE), "").unwrap();
        seed_defaults(&dir);
        assert_eq!(
            fs::read_to_string(dir.join("TM5.ini")).unwrap(),
            "[Settings]\r\nConfig File=1usmus v3 @ 1usmus.cfg\r\nMinutes=0\r\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn never_overwrites_a_choice() {
        let dir = temp("chosen");
        fs::write(dir.join("bin").join(DEFAULT_PROFILE), "").unwrap();
        fs::write(
            dir.join("TM5.ini"),
            "[Window]\r\nX=1\r\n[Settings]\r\nconfig file=Extreme @ anta777.cfg\r\n",
        )
        .unwrap();
        seed_defaults(&dir);
        assert_eq!(
            fs::read_to_string(dir.join("TM5.ini")).unwrap(),
            "[Window]\r\nX=1\r\n[Settings]\r\nMinutes=0\r\nconfig file=Extreme @ anta777.cfg\r\n"
        );
        // Nothing left to add: the file is left alone.
        let before = fs::read(dir.join("TM5.ini")).unwrap();
        seed_defaults(&dir);
        assert_eq!(fs::read(dir.join("TM5.ini")).unwrap(), before);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_the_profile_when_it_is_not_there() {
        let dir = temp("noprofile");
        seed_defaults(&dir);
        assert_eq!(
            fs::read_to_string(dir.join("TM5.ini")).unwrap(),
            "[Settings]\r\nMinutes=0\r\n"
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = (WORKING_NAME, WORKING_CFG);
    }
}
