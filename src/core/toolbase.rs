//! What every stress tool has in common, and the shape of a launch.
//!
//! Each tool in this program is a small adapter: it says where its executable
//! lives, what can be configured about it, and how a set of configured values
//! turns into a process to start. Everything after that -- running it, timing
//! it, watching it for failures, stopping it -- is the runner's job and is
//! written once.
//!
//! The adapters are deliberately thin. None of them reimplements a tool; they
//! write the configuration file or command line the tool already understands,
//! and that is all.
//!
//! A configuration is a JSON object (`Config`), as it was a dict in Python:
//! values are numbers, strings, booleans, and for Linpack Extended a list of
//! tests. The `cfg_*` helpers read it with Python's coercions.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::core::glob;
use crate::core::pyutil::{split_ws, strip};

pub type Config = Map<String, Value>;

/// Build a `Config` from `key => value` pairs.
#[macro_export]
macro_rules! config {
    () => { $crate::core::toolbase::Config::new() };
    ($($key:expr => $value:expr),+ $(,)?) => {{
        let mut map = $crate::core::toolbase::Config::new();
        $( map.insert(($key).to_string(), serde_json::json!($value)); )+
        map
    }};
}

// -- reading a config the way Python did ------------------------------------

/// `str(value)`, as Python would print it.
pub fn py_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Null => "None".into(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

/// Python truthiness.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().map(|n| n != 0.0).unwrap_or(false),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `int(value)`: numbers truncate, booleans are 0/1, strings are parsed.
/// None when Python would have raised.
pub fn py_int(value: &Value) -> Option<i64> {
    match value {
        Value::Bool(flag) => Some(*flag as i64),
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|f| f.trunc() as i64)),
        Value::String(text) => parse_int(text),
        _ => None,
    }
}

/// `int(text.strip())` for a decimal integer, saturating rather than
/// overflowing so a box full of nines clamps instead of resetting.
pub fn parse_int(text: &str) -> Option<i64> {
    let text = strip(text).replace('_', "");
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(&text)),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let magnitude = digits.trim_start_matches('0').parse::<i128>().unwrap_or(
        if digits.trim_start_matches('0').is_empty() {
            0
        } else {
            i128::MAX
        },
    );
    let signed = if negative { -magnitude } else { magnitude };
    Some(signed.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

/// `int(config.get(key, default) or 0)`.
pub fn cfg_int(config: &Config, key: &str, default: i64) -> i64 {
    match config.get(key) {
        None => default,
        Some(value) if !truthy(value) => 0,
        Some(value) => py_int(value).unwrap_or(0),
    }
}

/// `bool(config.get(key, default))`.
pub fn cfg_bool(config: &Config, key: &str, default: bool) -> bool {
    config.get(key).map(truthy).unwrap_or(default)
}

/// `str(config.get(key, default))`.
pub fn cfg_str(config: &Config, key: &str, default: &str) -> String {
    config
        .get(key)
        .map(py_str)
        .unwrap_or_else(|| default.to_string())
}

/// `dict(base, **extra)`: a copy of *base* with *extra* laid over it.
pub fn merged(base: &Config, extra: &Config) -> Config {
    let mut out = base.clone();
    for (key, value) in extra {
        out.insert(key.clone(), value.clone());
    }
    out
}

/// A path as the string a command line or a log shows.
pub fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

// -- fields and presets ------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Int,
    Text,
    Choice,
    Bool,
    /// Any combination of the choices, stored space-separated.
    Multi,
}

/// One configurable setting, and how to draw it.
///
/// A field is the only thing the UI needs to build a row, so adding a setting
/// to a tool means adding a Field and reading its key in `build` -- never
/// touching the window code.
#[derive(Debug, Clone)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub default: Value,
    pub choices: Vec<String>,
    pub hint: &'static str,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
    pub unit: &'static str,
}

impl Field {
    pub fn new(key: &'static str, label: &'static str, kind: FieldKind, default: Value) -> Self {
        Field {
            key,
            label,
            kind,
            default,
            choices: Vec::new(),
            hint: "",
            minimum: None,
            maximum: None,
            unit: "",
        }
    }

    pub fn int(key: &'static str, label: &'static str, default: i64) -> Self {
        Self::new(key, label, FieldKind::Int, Value::from(default))
    }

    pub fn boolean(key: &'static str, label: &'static str, default: bool) -> Self {
        Self::new(key, label, FieldKind::Bool, Value::from(default))
    }

    pub fn text(key: &'static str, label: &'static str, default: &str) -> Self {
        Self::new(key, label, FieldKind::Text, Value::from(default))
    }

    pub fn choice(key: &'static str, label: &'static str, default: &str, choices: &[&str]) -> Self {
        let mut field = Self::new(key, label, FieldKind::Choice, Value::from(default));
        field.choices = choices.iter().map(|c| c.to_string()).collect();
        field
    }

    pub fn multi(key: &'static str, label: &'static str, default: &str, choices: &[&str]) -> Self {
        let mut field = Self::new(key, label, FieldKind::Multi, Value::from(default));
        field.choices = choices.iter().map(|c| c.to_string()).collect();
        field
    }

    pub fn range(mut self, minimum: i64, maximum: i64) -> Self {
        self.minimum = Some(minimum);
        self.maximum = Some(maximum);
        self
    }

    pub fn hint(mut self, hint: &'static str) -> Self {
        self.hint = hint;
        self
    }

    pub fn unit(mut self, unit: &'static str) -> Self {
        self.unit = unit;
        self
    }

    /// Turn a value from an entry box into the value `build` expects.
    ///
    /// Out-of-range and unparseable values fall back to the default rather
    /// than failing: a typo in a memory box should not stop a run that is
    /// about to be left alone for six hours.
    pub fn coerce(&self, raw: &Value) -> Value {
        match self.kind {
            FieldKind::Int => {
                let Some(mut value) = parse_int(&py_str(raw)) else {
                    return self.default.clone();
                };
                if let Some(minimum) = self.minimum {
                    value = value.max(minimum);
                }
                if let Some(maximum) = self.maximum {
                    value = value.min(maximum);
                }
                Value::from(value)
            }
            FieldKind::Bool => {
                if let Value::Bool(flag) = raw {
                    return Value::Bool(*flag);
                }
                let text = strip(&py_str(raw)).to_lowercase();
                Value::Bool(matches!(text.as_str(), "1" | "true" | "yes" | "on"))
            }
            FieldKind::Multi => {
                let text = py_str(raw).replace(',', " ");
                let wanted: Vec<&str> = split_ws(&text);
                let chosen: Vec<&str> = self
                    .choices
                    .iter()
                    .map(|c| c.as_str())
                    .filter(|c| wanted.contains(c))
                    .collect();
                Value::from(chosen.join(" "))
            }
            FieldKind::Choice => {
                let text = py_str(raw);
                if self.choices.contains(&text) {
                    Value::from(text)
                } else {
                    self.default.clone()
                }
            }
            FieldKind::Text => Value::from(py_str(raw)),
        }
    }
}

/// A named set of field values, plus why you would pick it.
#[derive(Debug, Clone)]
pub struct Preset {
    pub name: String,
    pub values: Config,
    pub description: String,
}

impl Preset {
    pub fn new(name: &str, values: Config, description: &str) -> Self {
        Preset {
            name: name.to_string(),
            values,
            description: description.to_string(),
        }
    }
}

/// The one configuration a tool should run when nobody has chosen anything:
/// what the Quick Start page launches and what the tool's own tab opens on,
/// so there is exactly one default per tool and both places agree on it.
#[derive(Debug, Clone, Default)]
pub struct QuickStart {
    pub preset: Option<&'static str>,
    pub values: Config,
}

// -- launching ---------------------------------------------------------------

/// A setup step run on the runner's thread just after the process starts,
/// given its process id. Returns a line for the log, or an error -- and a run
/// whose setup failed is never started, because a stress test with settings
/// other than the ones asked for is worse than no test.
pub type OnStarted = Box<dyn FnOnce(u32) -> Result<String, String> + Send>;

/// Everything the runner needs to start and supervise one test.
#[derive(Default)]
pub struct LaunchSpec {
    pub argv: Vec<String>,
    /// A raw command line, used instead of argv when set. Needed only for
    /// Linpack Extended's teed console, a `cmd /c` line whose quoting no list
    /// of arguments survives being re-quoted into.
    pub cmdline: Option<String>,
    pub cwd: PathBuf,
    /// Variables set on top of this program's own environment.
    pub env: Vec<(String, String)>,
    /// True when the child writes to a pipe we can read. The windowed tools
    /// do not, and are watched through their log files instead.
    pub console: bool,
    pub watch_files: Vec<PathBuf>,
    pub error_key: String,
    pub summary: String,
    /// 0 means "until stopped". The runner enforces this itself, because
    /// only y-cruncher and Linpack can limit their own run time.
    pub duration_seconds: u64,
    pub creation_flags: u32,
    /// Lines that mean the tool has finished its work. Needed for any tool
    /// that does not exit when it is done: TestMem5 finishes its cycles,
    /// writes "Testing completed", and leaves its window open indefinitely.
    pub completion_patterns: Vec<String>,
    /// Lines that mean it stopped without finishing.
    pub abort_patterns: Vec<String>,
    /// Lines that mean the tool could not run for a reason that has nothing
    /// to do with the hardware. Reported as "could not start", never as a
    /// failure.
    pub setup_patterns: Vec<String>,
    /// Leave the tool on screen once it has finished by itself, instead of
    /// killing it the moment the verdict is known. Only applies to a run that
    /// ended on its own terms.
    pub leave_open: bool,
    pub on_started: Option<OnStarted>,
}

impl fmt::Debug for LaunchSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaunchSpec")
            .field("argv", &self.argv)
            .field("cmdline", &self.cmdline)
            .field("cwd", &self.cwd)
            .field("env", &self.env)
            .field("console", &self.console)
            .field("watch_files", &self.watch_files)
            .field("error_key", &self.error_key)
            .field("summary", &self.summary)
            .field("duration_seconds", &self.duration_seconds)
            .field(
                "creation_flags",
                &format_args!("{:#x}", self.creation_flags),
            )
            .field("completion_patterns", &self.completion_patterns)
            .field("abort_patterns", &self.abort_patterns)
            .field("leave_open", &self.leave_open)
            .finish()
    }
}

/// Why a tool could not be built into a launch: its files are missing, it
/// cannot run on this processor, or a configuration file could not be
/// written.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolError(pub String);

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ToolError {}

impl From<io::Error> for ToolError {
    fn from(error: io::Error) -> Self {
        ToolError(error.to_string())
    }
}

pub fn unavailable(message: impl Into<String>) -> ToolError {
    ToolError(message.into())
}

pub fn patterns(list: &[&str]) -> Vec<String> {
    list.iter().map(|p| p.to_string()).collect()
}

/// Give a console child its own visible window. Output cannot be piped at
/// the same time -- a redirected child writes to the pipe and its console
/// stays blank -- so a tool shown this way is watched through a file.
pub const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// Start a console child without flashing a console window, keeping its
/// output on our pipe and off the screen.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Write a text file with CRLF line endings, creating its folder -- the
/// equivalent of Python's `open(path, "w", newline="\r\n")`.
pub fn write_crlf(path: &Path, text: &str) -> io::Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
    fs::write(path, text)?;
    Ok(path.to_path_buf())
}

/// Delete a file left over from an earlier run, quietly.
pub fn remove_stale(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

/// Characters Windows will not have in a file name, removed, and the ends
/// trimmed of spaces and dots. "" when nothing usable is left.
pub fn clean_file_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|&c| {
            !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') && (c as u32) > 0x1f
        })
        .collect();
    strip(&kept).trim_matches('.').to_string()
}

// -- profiles ----------------------------------------------------------------

/// A tool that keeps its configurations as files -- y-cruncher's .bat files,
/// Linpack Extended's config.json copies -- each of which is also a Quick
/// Start button, and which its tab can make, edit and delete.
pub trait Profiles {
    /// (name, path) for each saved profile.
    fn profiles(&self, root: &Path) -> Vec<(String, PathBuf)>;
    /// A saved profile read back into the tab's settings.
    fn profile_values(&self, path: &Path) -> Result<Config, String>;
    fn clean_profile_name(&self, name: &str) -> String {
        clean_file_name(name)
    }
    fn save_profile(&self, root: &Path, name: &str, config: &Config) -> Result<PathBuf, ToolError>;
    /// What saving these settings would write: a command line, or a file.
    fn profile_preview(&self, config: &Config) -> String;
    fn profile_hint(&self) -> &'static str;
    /// A title for a preview big enough to need its own section, or "" for a
    /// one-line preview under the name.
    fn profile_preview_label(&self) -> &'static str {
        ""
    }
}

// -- the adapter -------------------------------------------------------------

/// A stress tool adapter.
pub trait Tool: Send + Sync {
    fn key(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn blurb(&self) -> &'static str;

    /// Relative to the tools root: the first path that exists wins, so a
    /// version bump that renames the folder only needs a pattern added.
    fn exe_globs(&self) -> &'static [&'static str] {
        &[]
    }

    /// Absolute patterns, searched after the tools root. Some of these tools
    /// are installed rather than unpacked.
    fn external_globs(&self) -> Vec<String> {
        Vec::new()
    }

    /// False for the windowed tools, whose output cannot be piped.
    fn console(&self) -> bool {
        false
    }

    /// Set on tools whose own window is the only place errors appear.
    fn detection_note(&self) -> &'static str {
        ""
    }

    fn fields(&self) -> Vec<Field>;

    fn presets(&self) -> Vec<Preset> {
        Vec::new()
    }

    fn quick_start(&self) -> QuickStart {
        QuickStart::default()
    }

    /// Whether this tool gets a tab of its own. False for a tool configured
    /// in its own window rather than here -- a tab of settings that do not
    /// reach it would be a tab that lies.
    fn has_tab(&self) -> bool {
        true
    }

    /// The tool's executable under *root*, or None.
    ///
    /// Matching is by glob so an unpacked folder keeps its version in the
    /// name without this program needing to know the number.
    fn locate(&self, root: &Path) -> Option<PathBuf> {
        default_locate(root, self.exe_globs(), &self.external_globs())
    }

    fn available(&self, root: &Path) -> bool {
        self.locate(root).is_some()
    }

    /// Why this tool cannot run on this machine, or "" when it can. Separate
    /// from `available`, which is about whether the files are there.
    fn unsupported_reason(&self, _root: &Path) -> String {
        String::new()
    }

    /// The field defaults as a config.
    fn defaults(&self) -> Config {
        self.fields()
            .into_iter()
            .map(|field| (field.key.to_string(), field.default))
            .collect()
    }

    /// Static presets, or ones discovered from the tool's own folder.
    fn all_presets(&self, _root: &Path) -> Vec<Preset> {
        self.presets()
    }

    /// The preset the quick-start default is built on, if any.
    fn quick_preset_name(&self, root: &Path) -> String {
        let names: Vec<String> = self.all_presets(root).into_iter().map(|p| p.name).collect();
        match self.quick_start().preset {
            Some(wanted) if names.iter().any(|n| n == wanted) => wanted.to_string(),
            _ => names.into_iter().next().unwrap_or_default(),
        }
    }

    /// A memory size suited to this machine for a preset, when the tool
    /// derives one.
    fn suggested_memory(&self, _preset: &str) -> Option<i64> {
        None
    }

    /// Whether this tool keeps derived figures in step with its memory box.
    fn has_apply_memory(&self) -> bool {
        false
    }

    /// Recompute figures derived from the memory box. Only meaningful when
    /// `has_apply_memory` is true.
    fn apply_memory(&self, _config: &mut Config) {}

    /// The tool's default configuration, fully resolved.
    ///
    /// Built from the field defaults, then the named preset, then the
    /// quick-start overrides -- in that order, so an override always wins.
    fn quick_config(&self, root: &Path) -> Config {
        let mut config = self.defaults();
        let name = self.quick_preset_name(root);
        if let Some(preset) = self.all_presets(root).into_iter().find(|p| p.name == name) {
            for (key, value) in preset.values {
                config.insert(key, value);
            }
        }
        if !name.is_empty() {
            if let Some(memory) = self.suggested_memory(&name) {
                config.insert("memory".into(), Value::from(memory));
            }
        }
        for (key, value) in self.quick_start().values {
            config.insert(key, value);
        }
        if self.has_apply_memory() {
            self.apply_memory(&mut config);
        }
        config
    }

    /// Field keys this preset does not let the user set.
    fn locked_fields(&self, _preset: &str) -> Vec<&'static str> {
        Vec::new()
    }

    /// The Quick Start buttons for this tool, as (label, config) pairs.
    fn quick_actions(&self, root: &Path) -> Vec<(String, Config)> {
        vec![("Start".to_string(), self.quick_config(root))]
    }

    /// A one-line description of the default, with no side effects.
    ///
    /// Deliberately not `build(..).summary`: building a spec writes
    /// configuration files, and the Quick Start page describes every tool the
    /// moment the window opens.
    fn quick_summary(&self, root: &Path) -> String {
        default_quick_summary(self, root)
    }

    /// What the card should say about how this run ends.
    fn duration_note(&self, config: &Config) -> String {
        let duration = cfg_int(config, "duration", 0);
        if duration != 0 {
            format!("{duration} min")
        } else {
            "no time limit".to_string()
        }
    }

    /// Turn configured values into a LaunchSpec.
    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, ToolError>;

    /// The profile files this tool keeps, when it keeps any.
    fn as_profiles(&self) -> Option<&dyn Profiles> {
        None
    }

    /// The saved profile a tab with profiles opens on, or "" for none.
    fn quick_profile_name(&self, _root: &Path) -> String {
        String::new()
    }

    /// True for Linpack Extended, whose tab edits a chain of tests.
    fn is_test_chain(&self) -> bool {
        false
    }
}

/// `Tool.quick_summary` as the base class wrote it, callable from overrides.
pub fn default_quick_summary<T: Tool + ?Sized>(tool: &T, root: &Path) -> String {
    let config = tool.quick_config(root);
    let mut parts = vec![tool.quick_preset_name(root)];
    let cycles = cfg_int(&config, "cycles", 0);
    if cycles != 0 {
        parts.push(format!(
            "{cycles}{}",
            if cycles != 1 { " cycles" } else { " cycle" }
        ));
    }
    let errors_limit = cfg_int(&config, "max_errors", 0);
    if errors_limit != 0 {
        parts.push(format!(
            "stop at {errors_limit} error{}",
            if errors_limit != 1 { "s" } else { "" }
        ));
    }
    parts.push(tool.duration_note(&config));
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("  |  ")
}

/// The summary every "opens its own window" card uses: the limit, or nothing.
pub fn limit_summary<T: Tool + ?Sized>(tool: &T, root: &Path) -> String {
    let limit = cfg_int(&tool.quick_config(root), "duration", 0);
    if limit != 0 {
        format!("{limit} min")
    } else {
        String::new()
    }
}

/// `Tool.locate`: the tools root first, sorted, first file wins; then the
/// absolute patterns, newest (last-sorting) first.
pub fn default_locate(root: &Path, globs: &[&str], external: &[String]) -> Option<PathBuf> {
    for pattern in globs {
        let mut matches = glob::glob_in(root, pattern, false);
        matches.sort();
        if let Some(found) = matches.into_iter().find(|m| m.is_file()) {
            return Some(found);
        }
    }
    for pattern in external {
        let mut matches: Vec<PathBuf> = glob::glob(pattern, false)
            .into_iter()
            .filter(|m| m.is_file())
            .collect();
        matches.sort();
        if let Some(found) = matches.pop() {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn int_fields_clamp_and_fall_back() {
        let field = Field::int("memory", "Memory", 4096).range(64, 1024);
        assert_eq!(field.coerce(&json!("512")), json!(512));
        assert_eq!(field.coerce(&json!(" 2000 ")), json!(1024));
        assert_eq!(field.coerce(&json!("1")), json!(64));
        assert_eq!(field.coerce(&json!("abc")), json!(4096));
        assert_eq!(field.coerce(&json!("")), json!(4096));
        assert_eq!(field.coerce(&json!("99999999999999999999999")), json!(1024));
        assert_eq!(field.coerce(&json!(300)), json!(300));
        assert_eq!(field.coerce(&json!("+100")), json!(100));
        assert_eq!(field.coerce(&json!("1.5")), json!(4096));
    }

    #[test]
    fn bool_choice_multi_text() {
        let flag = Field::boolean("pause", "Pause", false);
        assert_eq!(flag.coerce(&json!(true)), json!(true));
        assert_eq!(flag.coerce(&json!(" Yes ")), json!(true));
        assert_eq!(flag.coerce(&json!("off")), json!(false));
        let choice = Field::choice("priority", "Priority", "Normal", &["Normal", "High"]);
        assert_eq!(choice.coerce(&json!("High")), json!("High"));
        assert_eq!(choice.coerce(&json!("Realtime")), json!("Normal"));
        let multi = Field::multi(
            "algorithms",
            "Algorithms",
            "VSTv3",
            &["BKT", "SFTv4", "VSTv3"],
        );
        assert_eq!(
            multi.coerce(&json!("VSTv3, BKT nonsense")),
            json!("BKT VSTv3")
        );
        assert_eq!(multi.coerce(&json!("")), json!(""));
        let text = Field::text("extra", "Extra", "");
        assert_eq!(text.coerce(&json!(12)), json!("12"));
    }

    #[test]
    fn config_reads_follow_python() {
        let config = config! {
            "a" => 5, "b" => "7", "c" => false, "d" => "", "e" => true, "f" => 2.9
        };
        assert_eq!(cfg_int(&config, "a", 0), 5);
        assert_eq!(cfg_int(&config, "b", 0), 7);
        assert_eq!(cfg_int(&config, "c", 9), 0);
        assert_eq!(cfg_int(&config, "d", 9), 0);
        assert_eq!(cfg_int(&config, "e", 0), 1);
        assert_eq!(cfg_int(&config, "f", 0), 2);
        assert_eq!(cfg_int(&config, "missing", 3), 3);
        assert!(cfg_bool(&config, "e", false));
        assert!(!cfg_bool(&config, "d", true));
        assert!(cfg_bool(&config, "missing", true));
        assert_eq!(cfg_str(&config, "a", ""), "5");
        assert_eq!(cfg_str(&config, "c", ""), "False");
        assert_eq!(cfg_str(&config, "missing", "x"), "x");
    }

    #[test]
    fn file_names_are_cleaned() {
        assert_eq!(clean_file_name("  vt3: 28gb?  "), "vt3 28gb");
        assert_eq!(clean_file_name("..hidden.."), "hidden");
        assert_eq!(clean_file_name("a/b\\c|d*e\"f<g>h"), "abcdefgh");
        assert_eq!(clean_file_name("tab\there"), "tabhere");
        assert_eq!(clean_file_name("   "), "");
    }

    #[test]
    fn crlf_writing() {
        let dir = std::env::temp_dir().join(format!("roch-crlf-{}", std::process::id()));
        let path = dir.join("sub").join("x.txt");
        write_crlf(&path, "a\nb\r\nc\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"a\r\nb\r\nc\r\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
