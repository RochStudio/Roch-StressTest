//! Per-user settings and the directory runs write into.
//!
//! Kept in LOCALAPPDATA rather than beside the executable so the program works
//! from a read-only or Program Files install, and so a rebuild does not throw
//! away the settings. Roch Viewer stores its settings the same way.
//!
//! The location and the format -- `%LOCALAPPDATA%\RochStressTest\settings.json`,
//! a JSON object indented by two -- are exactly what the Python version wrote,
//! so settings saved by it are read here unchanged.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Map, Value};

pub const APP_DIR_NAME: &str = "RochStressTest";

/// Where the per-user directory lives instead of LOCALAPPDATA. Only ever set
/// by the tests, so that building every launch spec does not write into the
/// real settings of the person running them.
static OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Point every path in this module at *path* instead of LOCALAPPDATA.
pub fn set_app_data_override(path: Option<PathBuf>) {
    if let Ok(mut guard) = OVERRIDE.lock() {
        *guard = path;
    }
}

fn make_dir(path: PathBuf) -> PathBuf {
    let _ = fs::create_dir_all(&path);
    path
}

/// The per-user directory this program owns, created on demand.
pub fn app_data_dir() -> PathBuf {
    if let Ok(guard) = OVERRIDE.lock() {
        if let Some(path) = guard.as_ref() {
            return make_dir(path.clone());
        }
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    make_dir(base.join(APP_DIR_NAME))
}

/// A scratch directory for one tool's run artifacts.
///
/// Prime95 in particular insists on writing prime.txt, local.txt and
/// results.txt somewhere; pointing it here keeps its state out of the
/// distribution folder, which stays exactly as it was unpacked.
pub fn run_dir(name: &str) -> PathBuf {
    make_dir(app_data_dir().join("runs").join(name))
}

/// Where finished-run transcripts are kept.
pub fn log_dir() -> PathBuf {
    make_dir(app_data_dir().join("logs"))
}

pub fn settings_path() -> PathBuf {
    app_data_dir().join("settings.json")
}

/// Return the saved settings, or an empty map when there are none.
pub fn load_settings() -> Map<String, Value> {
    let Ok(text) = fs::read_to_string(settings_path()) else {
        return Map::new();
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// Merge one setting into the file, leaving the others alone.
pub fn save_setting(key: &str, value: Value) {
    let mut settings = load_settings();
    settings.insert(key.to_string(), value);
    let text = match serde_json::to_string_pretty(&Value::Object(settings)) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("Could not save setting {key}: {error}");
            return;
        }
    };
    if let Err(error) = fs::write(settings_path(), text) {
        eprintln!("Could not save setting {key}: {error}");
    }
}

/// "Dark" or "Light"; anything else saved is treated as Dark.
pub fn load_appearance_mode() -> String {
    appearance_from(load_settings().get("appearance_mode"))
}

fn appearance_from(value: Option<&Value>) -> String {
    let text = match value {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => "Dark".to_string(),
    };
    match text.to_lowercase().as_str() {
        "light" => "Light".to_string(),
        _ => "Dark".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn appearance_mode_is_normalised() {
        assert_eq!(appearance_from(None), "Dark");
        assert_eq!(appearance_from(Some(&json!("light"))), "Light");
        assert_eq!(appearance_from(Some(&json!("LIGHT"))), "Light");
        assert_eq!(appearance_from(Some(&json!("Dark"))), "Dark");
        assert_eq!(appearance_from(Some(&json!("System"))), "Dark");
        assert_eq!(appearance_from(Some(&json!(5))), "Dark");
    }
}
