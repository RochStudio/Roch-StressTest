//! y-cruncher's component stress tester.
//!
//! The best-behaved tool of the set: a real command line, a real time limit,
//! and a `logfile:` option that mirrors everything it prints. That logfile is
//! what lets it run in a visible console: a child whose stdout is redirected
//! leaves its own console blank, so a tool cannot be both piped and
//! watchable -- but y-cruncher will write the same text to a file, which the
//! runner tails instead.
//!
//! The algorithm names come straight from Command Lines.txt in the
//! distribution. Naming any algorithm disables the rest, which is why ticking
//! none of them passes none at all rather than listing them: that is how
//! y-cruncher is told to run the lot.

use std::fs;
use std::path::{Path, PathBuf};

use fancy_regex::Regex;
use serde_json::Value;

use crate::config;
use crate::core::errors;
use crate::core::glob;
use crate::core::pyutil::{decode_ansi, encode_ansi, split_ws, splitlines, strip};
use crate::core::settings;
use crate::core::toolbase::{
    cfg_bool, cfg_int, cfg_str, default_quick_summary, merged, parse_int, path_str, patterns,
    remove_stale, truthy, unavailable, Config, Field, LaunchSpec, Profiles, QuickStart, Tool,
    ToolError, CREATE_NEW_CONSOLE, CREATE_NO_WINDOW,
};

pub struct YCruncher;

/// The valid values for [algorithm], in the order the manual lists them. N63,
/// N64 and VST are aliases y-cruncher still accepts; only the canonical names
/// are offered here, so nothing saved depends on an alias surviving.
pub const ALGORITHMS: &[(&str, &str)] = &[
    ("BKT", "Basecase + Karatsuba + Toom-Cook"),
    ("BBP", "Bailey-Borwein-Plouffe digit extraction"),
    ("SFTv4", "Small in-cache FFT"),
    ("SNT", "Small in-cache NTT"),
    ("SVT", "Small in-cache vector transform"),
    ("FFTv4", "Fast Fourier transform"),
    ("NTT63", "Classic 64-bit NTT"),
    ("VSTv3", "Vector-scalable transform"),
];

/// Every spelling y-cruncher accepts, mapped to the name offered here, so a
/// hand-typed VT3 in a .bat is read back as the VSTv3 tick it means.
pub fn canonical_algorithm(token: &str) -> Option<&'static str> {
    let upper = token.to_uppercase();
    if let Some((name, _)) = ALGORITHMS
        .iter()
        .find(|(name, _)| name.to_uppercase() == upper)
    {
        return Some(name);
    }
    match upper.as_str() {
        "VT3" | "VST" => Some("VSTv3"),
        "N63" | "N64" => Some("NTT63"),
        "SFT" => Some("SFTv4"),
        "FFT" => Some("FFTv4"),
        _ => None,
    }
}

/// Windows priority values from the manual's Startup Parameters section.
pub const PRIORITIES: &[(&str, i64)] = &[
    ("Below normal", -1),
    ("Normal", 0),
    ("Above normal", 1),
    ("High", 2),
];

/// What y-cruncher runs at when a .bat says nothing about priority, so a
/// profile saved at this priority does not need to say it either.
pub const NATIVE_PRIORITY: &str = "Below normal";

/// How much longer than its own -TL y-cruncher is given before the runner
/// steps in. -TL is only checked between tests, so y-cruncher routinely runs
/// past its limit to finish the test in progress, and a backstop that fired
/// on the dot would kill it mid-test every time.
pub const BACKSTOP_GRACE_SECONDS: u64 = 300;

pub const PROFILE_HINT: &str =
    "Saved as <name>.bat beside y-cruncher.exe, and shown as a button on Quick Start.";

fn memory_argument(gigabytes: i64) -> String {
    format!("-M:{gigabytes}GB")
}

/// The y-cruncher arguments these settings make, as a .bat holds them.
///
/// Everything before "stress" is a startup parameter and everything after it
/// belongs to the stress test, per the manual. Nothing is written that
/// y-cruncher would do anyway, so a profile says only what was chosen.
pub fn profile_arguments(config: &Config) -> Vec<String> {
    let mut startup: Vec<String> = Vec::new();
    if config.get("pause").map(truthy).unwrap_or(false) {
        startup.push("pause:1".into());
    }
    if let Some(Value::String(priority)) = config.get("priority") {
        if let Some((_, number)) = PRIORITIES.iter().find(|(label, _)| label == priority) {
            if priority != NATIVE_PRIORITY {
                startup.push(format!("priority:{number}"));
            }
        }
    }

    let mut test: Vec<String> = Vec::new();
    let memory = cfg_int(config, "memory", 0);
    if memory > 0 {
        test.push(memory_argument(memory));
    }
    let per_test = cfg_int(config, "per_test", 0);
    if per_test > 0 {
        test.push(format!("-D:{per_test}"));
    }
    let minutes = cfg_int(config, "duration", 0);
    if minutes > 0 {
        test.push(format!("-TL:{}", minutes * 60));
    }

    // Extras go on whichever side of "stress" they are shaped for: a startup
    // parameter is "name:value" with no leading dash.
    let extra = cfg_str(config, "extra", "");
    for token in split_ws(&extra) {
        if token.starts_with('-') || !token.contains(':') {
            test.push(token.to_string());
        } else {
            startup.push(token.to_string());
        }
    }

    let chosen = cfg_str(config, "algorithms", "").replace(',', " ");
    let mut selected: Vec<String> = Vec::new();
    for token in split_ws(&chosen) {
        if let Some(canonical) = canonical_algorithm(token) {
            if !selected.iter().any(|s| s == canonical) {
                selected.push(canonical.to_string());
            }
        }
    }
    let mut arguments = startup;
    arguments.push("stress".into());
    arguments.extend(test);
    arguments.extend(selected);
    arguments
}

pub fn profile_preview(config: &Config) -> String {
    let mut parts = vec!["y-cruncher.exe".to_string()];
    parts.extend(profile_arguments(config));
    parts.join(" ")
}

/// -M's value in whole gigabytes, or None when it cannot be read.
pub fn gigabytes(text: &str) -> Option<i64> {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r"^(?:(\d+(?:\.\d+)?)([KMGT]?)(?:I?B)?)$").expect("gigabytes pattern")
    });
    let upper = text.to_uppercase();
    let captures = pattern.captures(&upper).ok()??;
    let number: f64 = captures.get(1)?.as_str().parse().ok()?;
    let scale = match captures.get(2).map(|m| m.as_str()).unwrap_or("") {
        "" => 1024f64.powi(-3),
        "K" => 1024f64.powi(-2),
        "M" => 1024f64.powi(-1),
        "G" => 1.0,
        "T" => 1024.0,
        _ => return None,
    };
    Some(((number * scale).round_ties_even() as i64).max(1))
}

/// The y-cruncher arguments out of a .bat, or nothing if there are none.
///
/// These files are one line calling y-cruncher.exe, which is all this needs
/// to understand. Anything more elaborate is left alone rather than
/// half-read.
pub fn bat_arguments(path: &Path) -> Vec<String> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    let text = decode_ansi(&bytes);
    for line in splitlines(&text) {
        let stripped = strip(line);
        let lower = stripped.to_lowercase();
        if lower.contains("y-cruncher") && lower.contains(".exe") {
            let parts = split_ws(stripped);
            for (index, part) in parts.iter().enumerate() {
                let part = part.to_lowercase();
                if part.ends_with("y-cruncher.exe") || part.ends_with("y-cruncher.exe\"") {
                    return parts[index + 1..].iter().map(|p| p.to_string()).collect();
                }
            }
        }
    }
    Vec::new()
}

/// A .bat read back into the tab's settings.
///
/// Anything the tab has no box for is kept in "Other options" rather than
/// dropped, so opening a profile and saving it again never loses part of it.
pub fn values_from_arguments(arguments: &[String]) -> Config {
    let mut values = config! {
        "algorithms" => "", "memory" => 0, "per_test" => 0, "duration" => 0,
        "pause" => false, "priority" => NATIVE_PRIORITY, "extra" => ""
    };
    let mut extra: Vec<String> = Vec::new();
    let mut selected: Vec<&'static str> = Vec::new();
    for token in arguments {
        let lower = token.to_lowercase();
        let (name, value) = match token.split_once(':') {
            Some((name, value)) => (name, value),
            None => (token.as_str(), ""),
        };
        if lower == "stress" {
            continue;
        }
        if lower.starts_with("pause:") {
            let value = strip(value);
            values.insert("pause".into(), Value::Bool(value == "1"));
            if value != "1" && value != "-2" {
                extra.push(token.clone());
            }
            continue;
        }
        if lower.starts_with("priority:") {
            let value = strip(value);
            match PRIORITIES
                .iter()
                .find(|(_, number)| number.to_string() == value)
            {
                Some((label, _)) => {
                    values.insert("priority".into(), Value::from(*label));
                }
                None => extra.push(token.clone()),
            }
            continue;
        }
        let upper_name = name.to_uppercase();
        if matches!(upper_name.as_str(), "-M" | "-D" | "-TL") {
            let number = if upper_name == "-M" {
                gigabytes(value)
            } else if !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) {
                value.parse::<i64>().ok()
            } else {
                None
            };
            match number {
                None => extra.push(token.clone()),
                Some(number) if upper_name == "-M" => {
                    values.insert("memory".into(), Value::from(number));
                }
                Some(number) if upper_name == "-D" => {
                    values.insert("per_test".into(), Value::from(number));
                }
                Some(number) => {
                    let minutes = (number as f64 / 60.0).round_ties_even() as i64;
                    values.insert("duration".into(), Value::from(minutes.max(1)));
                }
            }
            continue;
        }
        if let Some(canonical) = canonical_algorithm(token) {
            if !selected.contains(&canonical) {
                selected.push(canonical);
            }
            continue;
        }
        extra.push(token.clone());
    }
    values.insert("algorithms".into(), Value::from(selected.join(" ")));
    values.insert("extra".into(), Value::from(extra.join(" ")));
    values
}

impl YCruncher {
    /// Every .bat sitting beside y-cruncher.exe, sorted by name.
    ///
    /// Read from the folder rather than listed here, so a .bat dropped in
    /// beside it gets a button without this file being touched.
    pub fn bat_files(&self, root: &Path) -> Vec<PathBuf> {
        let Some(exe) = self.locate(root) else {
            return Vec::new();
        };
        let Some(folder) = exe.parent() else {
            return Vec::new();
        };
        let mut found = glob::glob_in(folder, "*.bat", false);
        found.sort_by_key(|path| file_name(path).to_lowercase());
        found
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Profiles for YCruncher {
    fn profiles(&self, root: &Path) -> Vec<(String, PathBuf)> {
        self.bat_files(root)
            .into_iter()
            .map(|path| (file_stem(&path), path))
            .collect()
    }

    fn profile_values(&self, path: &Path) -> Result<Config, String> {
        Ok(values_from_arguments(&bat_arguments(path)))
    }

    fn save_profile(&self, root: &Path, name: &str, config: &Config) -> Result<PathBuf, ToolError> {
        let exe = self.locate(root);
        let name = self.clean_profile_name(name);
        let (Some(exe), false) = (exe, name.is_empty()) else {
            return Err(unavailable("A profile needs a name and y-cruncher."));
        };
        let path = exe
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!("{name}.bat"));
        let line = profile_preview(config);
        fs::write(&path, encode_ansi(&format!("{line}\r\n")))?;
        Ok(path)
    }

    fn profile_preview(&self, config: &Config) -> String {
        profile_preview(config)
    }

    fn profile_hint(&self) -> &'static str {
        PROFILE_HINT
    }
}

impl Tool for YCruncher {
    fn key(&self) -> &'static str {
        "ycruncher"
    }
    fn name(&self) -> &'static str {
        "y-cruncher"
    }
    fn blurb(&self) -> &'static str {
        "Component stress tester. VSTv3 and FFTv4 are the AVX-heavy pair that finds an unstable \
         memory clock quickly; the small in-cache tests stay on the core."
    }
    fn exe_globs(&self) -> &'static [&'static str] {
        &["y-cruncher*/y-cruncher.exe", "y-cruncher.exe"]
    }
    fn console(&self) -> bool {
        true
    }
    fn fields(&self) -> Vec<Field> {
        let names: Vec<&str> = ALGORITHMS.iter().map(|(name, _)| *name).collect();
        let priorities: Vec<&str> = PRIORITIES.iter().map(|(label, _)| *label).collect();
        vec![
            Field::multi("algorithms", "Algorithms", "VSTv3", &names).hint(
                "Tick as many as you want; they run one after another. None ticked runs all of \
                 them, which is y-cruncher's own default.",
            ),
            Field::int("memory", "Memory to use", 0)
                .range(0, 1024)
                .unit("GB")
                .hint("-M. 0 leaves it out and y-cruncher picks."),
            Field::int("per_test", "Seconds per test", 60)
                .range(0, 86_400)
                .unit("s")
                .hint(
                    "-D. How long each algorithm runs before the next. 0 leaves it out and \
                     y-cruncher picks.",
                ),
            Field::int("duration", "Stop after", 60)
                .range(0, 100_000)
                .unit("min")
                .hint(
                    "-TL, in seconds on the command line. y-cruncher checks it between tests, so \
                     it stops at the first test boundary at or after this; the countdown allows \
                     five minutes past it before stepping in. 0 runs until you press Stop.",
                ),
            Field::boolean("pause", "Pause at the end", false)
                .hint("pause:1. Keeps the finished result on screen."),
            Field::choice("priority", "Process priority", "Normal", &priorities).hint(
                "priority:. y-cruncher runs at below normal when a profile does not say, which \
                 shares the machine but under-stresses it.",
            ),
            Field::text("extra", "Other options", "").hint(
                "Anything else to pass, exactly as y-cruncher takes it. Options before 'stress' \
                 in a .bat that this tab has no box for are kept here too.",
            ),
            Field::boolean("show_window", "Show y-cruncher's window", true).hint(
                "Only for Start on this tab; a saved .bat always shows it. Failures are read \
                 from its log file either way.",
            ),
        ]
    }

    /// What the Quick Start page runs, and what this tab opens on.
    fn quick_start(&self) -> QuickStart {
        QuickStart {
            preset: None,
            values: config! {
                "algorithms" => "VSTv3", "memory" => 28, "duration" => 30,
                "per_test" => 0, "pause" => true
            },
        }
    }

    /// Open the menu, or run one of the .bat files beside it.
    fn quick_actions(&self, root: &Path) -> Vec<(String, Config)> {
        let base = self.quick_config(root);
        let mut actions = vec![(
            "Open".to_string(),
            merged(&base, &config! {"quick_mode" => "open"}),
        )];
        for bat in self.bat_files(root) {
            let label = file_stem(&bat).to_uppercase();
            actions.push((
                label,
                merged(
                    &base,
                    &config! {"quick_mode" => "bat", "bat_path" => path_str(&bat)},
                ),
            ));
        }
        actions
    }

    fn quick_summary(&self, root: &Path) -> String {
        let names: Vec<String> = self
            .bat_files(root)
            .iter()
            .map(|b| file_stem(b).to_uppercase())
            .collect();
        if names.is_empty() {
            return default_quick_summary(self, root);
        }
        format!("Open the menu, or run {}", names.join(" / "))
    }

    fn as_profiles(&self) -> Option<&dyn Profiles> {
        Some(self)
    }

    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, ToolError> {
        let exe = self.locate(root).ok_or_else(|| {
            unavailable(
                "y-cruncher.exe was not found. Expected a folder like 'y-cruncher v0.8.7.9547b' \
                 beside this program.",
            )
        })?;
        let folder = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        let exe_text = path_str(&exe);

        let work = settings::run_dir("ycruncher");
        let mode = cfg_str(config, "quick_mode", "");

        if mode == "open" {
            // No arguments at all: y-cruncher's own menu, which is what it
            // does when it is double-clicked. Nothing is watched because
            // nothing has been chosen yet.
            return Ok(LaunchSpec {
                argv: vec![exe_text],
                cwd: folder,
                console: false,
                error_key: self.key().into(),
                summary: "y-cruncher (its own menu)".into(),
                duration_seconds: 0,
                leave_open: true,
                creation_flags: CREATE_NEW_CONSOLE,
                ..Default::default()
            });
        }

        let logfile = work.join("ycruncher.log");
        let added = vec![
            "skip-warnings".to_string(),
            format!("logfile:{}", path_str(&logfile)),
        ];

        if mode == "bat" {
            let bat = PathBuf::from(cfg_str(config, "bat_path", ""));
            let arguments = bat_arguments(&bat);
            if arguments.is_empty() {
                return Err(unavailable(format!(
                    "{} has no y-cruncher command in it that this could read.",
                    file_name(&bat)
                )));
            }

            // The .bat decides the test. logfile: and skip-warnings are added
            // and nothing is taken away. Startup parameters have to come
            // before "stress", per the manual, so they are inserted rather
            // than appended. colors:0 is deliberately NOT added: the logfile
            // is written plain either way, and all it did was take the colour
            // off the window somebody is watching.
            let mut argv = vec![exe_text];
            match arguments.iter().position(|a| a == "stress") {
                Some(cut) => {
                    argv.extend(arguments[..cut].iter().cloned());
                    argv.extend(added.iter().cloned());
                    argv.extend(arguments[cut..].iter().cloned());
                }
                None => {
                    argv.extend(added.iter().cloned());
                    argv.extend(arguments.iter().cloned());
                }
            }

            // -TL: is y-cruncher's own limit; the runner's clock allows five
            // minutes past it before stepping in.
            let mut seconds = 0u64;
            for part in &arguments {
                if part.to_lowercase().starts_with("-tl:") {
                    let value = part.split_once(':').map(|(_, v)| v).unwrap_or("");
                    seconds = match parse_int(value) {
                        Some(limit) => (limit + BACKSTOP_GRACE_SECONDS as i64).max(0) as u64,
                        None => 0,
                    };
                }
            }

            remove_stale(&logfile);

            return Ok(LaunchSpec {
                argv,
                cwd: folder,
                console: false,
                watch_files: vec![logfile],
                error_key: self.key().into(),
                summary: format!("y-cruncher {}: {}", file_name(&bat), arguments.join(" ")),
                duration_seconds: seconds,
                leave_open: true,
                creation_flags: CREATE_NEW_CONSOLE,
                ..Default::default()
            });
        }

        // Exactly the command line a profile saved from this tab would hold,
        // so Start and the .bat it saves always run the same test. Added to
        // it, before "stress": skip-warnings, without which y-cruncher waits
        // at a startup prompt for ENTER that nobody is there to press, and
        // pause:-2 when pause is off, so it does not wait on its own default.
        let arguments = profile_arguments(config);
        let mut added = added;
        if !cfg_bool(config, "pause", false) {
            added.insert(0, "pause:-2".into());
        }
        let cut = arguments.iter().position(|a| a == "stress").unwrap_or(0);
        let mut argv = vec![exe_text];
        argv.extend(arguments[..cut].iter().cloned());
        argv.extend(added);
        argv.extend(arguments[cut..].iter().cloned());

        let memory = cfg_int(config, "memory", 0);
        let per_test = cfg_int(config, "per_test", 0);
        let duration_seconds = cfg_int(config, "duration", 0).max(0) as u64 * 60;
        let selected: Vec<&str> = arguments[cut..]
            .iter()
            .map(|a| a.as_str())
            .filter(|a| ALGORITHMS.iter().any(|(name, _)| name == a))
            .collect();

        remove_stale(&logfile);

        // Shown in its own console, or piped and hidden -- never both.
        let show = cfg_bool(config, "show_window", true);
        let summary = format!(
            "y-cruncher {}, {}{}",
            if selected.is_empty() {
                "all tests".to_string()
            } else {
                selected.join(" ")
            },
            if memory != 0 {
                format!("{memory} GB")
            } else {
                "default memory".to_string()
            },
            if per_test > 0 {
                format!(", {per_test}s per test")
            } else {
                ", default test length".to_string()
            }
        );

        Ok(LaunchSpec {
            argv,
            cwd: folder,
            console: !show,
            watch_files: if show { vec![logfile] } else { Vec::new() },
            error_key: self.key().into(),
            summary,
            // The backstop, not the limit. -TL is what actually ends the run.
            duration_seconds: if duration_seconds != 0 {
                duration_seconds + BACKSTOP_GRACE_SECONDS
            } else {
                0
            },
            creation_flags: if show {
                CREATE_NEW_CONSOLE
            } else {
                CREATE_NO_WINDOW
            },
            // pause:1 holds the window open at the end. That is only visible
            // to anyone if the run does not then kill it.
            leave_open: show && cfg_bool(config, "pause", false),
            // A clean exit without this line means the run ended early.
            completion_patterns: patterns(errors::YCRUNCHER_COMPLETE),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn quick_default_command_line() {
        let config = merged(&YCruncher.defaults(), &YCruncher.quick_start().values);
        assert_eq!(
            profile_preview(&config),
            "y-cruncher.exe pause:1 priority:0 stress -M:28GB -TL:1800 VSTv3"
        );
    }

    #[test]
    fn arguments_put_startup_parameters_before_stress() {
        let config = config! {
            "algorithms" => "vt3, FFTv4 VSTv3 bogus", "memory" => 4, "per_test" => 60,
            "duration" => 0, "pause" => false, "priority" => "Below normal",
            "extra" => "colors:0 -X 7 thing:1"
        };
        assert_eq!(
            profile_arguments(&config),
            args(&[
                "colors:0", "thing:1", "stress", "-M:4GB", "-D:60", "-X", "7", "VSTv3", "FFTv4"
            ])
        );
        assert_eq!(profile_arguments(&Config::new()), args(&["stress"]));
    }

    #[test]
    fn bats_read_back_into_settings() {
        let values = values_from_arguments(&args(&[
            "pause:1",
            "priority:0",
            "stress",
            "-M:28GB",
            "-D:60",
            "-TL:216000",
            "VSTv3",
        ]));
        assert_eq!(
            Value::Object(values),
            serde_json::json!({
                "algorithms": "VSTv3", "memory": 28, "per_test": 60, "duration": 3600,
                "pause": true, "priority": "Normal", "extra": ""
            })
        );
        let values = values_from_arguments(&args(&[
            "pause:0",
            "priority:9",
            "colors:0",
            "stress",
            "-TL:1800",
            "-M:512MiB",
            "-D:x",
            "VT3",
            "N64",
            "SFT",
        ]));
        assert_eq!(values["pause"], Value::Bool(false));
        assert_eq!(values["priority"], Value::from(NATIVE_PRIORITY));
        assert_eq!(values["duration"], Value::from(30));
        assert_eq!(values["memory"], Value::from(1));
        assert_eq!(values["algorithms"], Value::from("VSTv3 NTT63 SFTv4"));
        assert_eq!(
            values["extra"],
            Value::from("pause:0 priority:9 colors:0 -D:x")
        );
        // -TL under a minute still reads as one minute.
        assert_eq!(
            values_from_arguments(&args(&["-TL:10"]))["duration"],
            Value::from(1)
        );
    }

    #[test]
    fn a_profile_survives_a_round_trip() {
        let config = config! {
            "algorithms" => "BKT SVT", "memory" => 16, "per_test" => 120, "duration" => 45,
            "pause" => true, "priority" => "High", "extra" => "colors:0 -Z"
        };
        let back = values_from_arguments(&profile_arguments(&config));
        for key in [
            "algorithms",
            "memory",
            "per_test",
            "duration",
            "pause",
            "priority",
        ] {
            assert_eq!(back[key], config[key], "{key}");
        }
        assert_eq!(back["extra"], Value::from("colors:0 -Z"));
    }

    #[test]
    fn memory_sizes() {
        assert_eq!(gigabytes("28GB"), Some(28));
        assert_eq!(gigabytes("28g"), Some(28));
        assert_eq!(gigabytes("1.5T"), Some(1536));
        assert_eq!(gigabytes("2048M"), Some(2));
        assert_eq!(gigabytes("100"), Some(1));
        assert_eq!(gigabytes("2.5GIB"), Some(2));
        assert_eq!(gigabytes("3.5GB"), Some(4));
        assert_eq!(gigabytes("lots"), None);
        assert_eq!(gigabytes(""), None);
    }

    #[test]
    fn bat_parsing() {
        let dir = std::env::temp_dir().join(format!("roch-yc-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let bat = dir.join("vt3 28gb.bat");
        fs::write(&bat, "@echo off\r\nrem y-cruncher.exe is called below\r\n\"C:\\x\\y-cruncher.exe\" pause:1 stress -TL:1800 VT3\r\n").unwrap();
        // The rem line mentions the exe but no token ends with it as a word
        // followed by arguments... it does: "y-cruncher.exe" -> the rest.
        assert_eq!(bat_arguments(&bat), args(&["is", "called", "below"]));
        fs::write(&bat, "y-cruncher.exe pause:1 stress -TL:1800 VT3").unwrap();
        assert_eq!(
            bat_arguments(&bat),
            args(&["pause:1", "stress", "-TL:1800", "VT3"])
        );
        fs::write(&bat, "\"C:\\x\\y-cruncher.exe\" stress").unwrap();
        assert_eq!(bat_arguments(&bat), args(&["stress"]));
        fs::write(&bat, "echo nothing").unwrap();
        assert!(bat_arguments(&bat).is_empty());
        assert!(bat_arguments(&dir.join("missing.bat")).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
