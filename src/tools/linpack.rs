//! The two Linpack packages: Linpack Xtreme, opened at its own menu, and
//! Linpack Extended, run through its own Node driver with a config made here.
//!
//! Both are front-ends around the same Intel benchmark binary. The input file
//! format, and the trial count of 99999 that turns a fixed number of runs into
//! "until we stop it", are the Node driver's, in
//! Linpack-Extended-master/dependencies/linpack.js.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::config;
use crate::core::glob;
use crate::core::hardware;
use crate::core::settings;
use crate::core::toolbase::{
    cfg_bool, cfg_int, cfg_str, limit_summary, merged, path_str, patterns, py_int, py_str,
    remove_stale, truthy, unavailable, write_crlf, Config, Field, LaunchSpec, Profiles, QuickStart,
    Tool, ToolError, CREATE_NEW_CONSOLE, CREATE_NO_WINDOW,
};

/// cmd has no tee, and Linpack Extended has no log option, so its output is
/// split by this: on to the console it is running in, and into a file the
/// runner can read.
pub const TEE_JS: &str = "// Written by Roch StressTest. cmd has no tee, and Linpack Extended has\n// no log option, so its output is split here: on to the console it is\n// running in, and into a file the runner can read. node is used because\n// the package already ships it -- adding a binary for this would be\n// worse, and PowerShell is what the console was moved away from.\nconst fs = require(\"fs\");\nconst out = fs.createWriteStream(process.argv[2]);\nprocess.stdin.on(\"data\", function (chunk) {\n  process.stdout.write(chunk);\n  out.write(chunk);\n});\nprocess.stdin.on(\"end\", function () { out.end(); });\n";

/// The leading dimension Intel's documentation asks for.
///
/// From "leading dimensions.html" in the Linpack Extended package: best
/// performance comes from the nearest odd multiple of 8 at or above the
/// problem size -- 16 on AVX parts, meaning divisible by 16 but not by 32.
pub fn leading_dimension(problem_size: i64, avx: bool) -> i64 {
    let step = if avx { 16 } else { 8 };
    let mut lda = (problem_size + step - 1).div_euclid(step) * step;
    if lda.rem_euclid(step * 2) == 0 {
        lda += step;
    }
    lda
}

/// The largest problem that fits in *memory_mb*.
///
/// The matrix alone needs 8 * lda * n bytes, so n starts at sqrt(bytes/8) and
/// steps down until the padded leading dimension fits as well.
pub fn problem_size_for(memory_mb: f64, avx: bool) -> i64 {
    let target = (memory_mb.trunc() as i64).max(1) * 1024 * 1024;
    let mut size = (target as f64 / 8.0).sqrt() as i64;
    let step = if avx { 16 } else { 8 };
    size -= size % step;
    while size > step {
        if 8 * leading_dimension(size, avx) * size <= target {
            return size;
        }
        size -= step;
    }
    step
}

/// Recompute problem size and leading dimension from the memory box, so the
/// three stay consistent without the user doing the arithmetic.
fn apply_memory(config: &mut Config) {
    let memory = config.get("memory").and_then(py_int).unwrap_or(4096);
    let size = problem_size_for(memory as f64, true);
    config.insert("problem_size".into(), Value::from(size));
    config.insert(
        "leading_dimension".into(),
        Value::from(leading_dimension(size, true)),
    );
}

// -- Linpack Xtreme ----------------------------------------------------------

/// Linpack Xtreme, opened at its own menu.
///
/// The package is a console front-end around the Intel binary: it asks how
/// much memory, how many trials and how long, then runs it and prints the
/// table. Those questions are the whole configuration, and they are asked
/// far more clearly there than a tab of the same fields would be.
pub struct LinpackXtreme;

impl Tool for LinpackXtreme {
    fn key(&self) -> &'static str {
        "linpack_xtreme"
    }
    fn name(&self) -> &'static str {
        "Linpack Xtreme"
    }
    fn blurb(&self) -> &'static str {
        "The heaviest sustained AVX load of the set, and the quickest way to find a core or \
         memory setting that is only nearly stable. This is the package to use on AMD -- it \
         ships a build that runs there. Watch temperatures: nothing else here pulls this much \
         current."
    }
    // The menu, not the benchmark binary underneath it. It picks the right
    // build for the processor itself.
    fn exe_globs(&self) -> &'static [&'static str] {
        &[
            "LinpackXtreme*/LinpackXtreme_x64.exe",
            "LinpackXtreme*/LinpackXtreme_x32.exe",
        ]
    }
    fn console(&self) -> bool {
        true
    }
    fn detection_note(&self) -> &'static str {
        LINPACK_DETECTION_NOTE
    }
    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0)
            .range(0, 100_000)
            .unit("min")
            .hint("0 lets the trials you asked it for finish in their own time.")]
    }
    // Answered in its own window, so there is no tab here.
    fn has_tab(&self) -> bool {
        false
    }
    fn quick_start(&self) -> QuickStart {
        QuickStart {
            preset: None,
            values: config! {"duration" => 0},
        }
    }
    fn has_apply_memory(&self) -> bool {
        true
    }
    fn apply_memory(&self, config: &mut Config) {
        apply_memory(config)
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
                "LinpackXtreme_x64.exe was not found. Expected its folder beside this program.",
            )
        })?;
        // Its own console, and nothing read from it. The front-end prints the
        // table to the screen and keeps no log; what the runner still does is
        // hold it to a limit if one was set and notice if it dies.
        Ok(LaunchSpec {
            argv: vec![path_str(&exe)],
            cwd: exe.parent().map(Path::to_path_buf).unwrap_or_default(),
            console: false,
            error_key: self.key().into(),
            summary: "Linpack Xtreme (settings answered at its menu)".into(),
            duration_seconds: cfg_int(config, "duration", 0).max(0) as u64 * 60,
            leave_open: true,
            creation_flags: CREATE_NEW_CONSOLE,
            ..Default::default()
        })
    }
}

pub const LINPACK_DETECTION_NOTE: &str =
    "Every result row is parsed. A row whose check column is not 'pass', or whose residual \
     drifts from the first trial, stops the test. Linpack itself would have carried on.";

// -- Linpack Extended --------------------------------------------------------

/// Linpack Extended's binary, which is Intel-only.
///
/// Its linpack_xeon64.exe checks the vendor string and, on anything that is
/// not an Intel processor, prints "This binary version of the SMP LINPACK
/// benchmark is optimized for and runs on only genuine Intel processors" and
/// exits with status zero. Nothing is tested and nothing looks wrong, which is
/// why this tool refuses to start on AMD rather than letting it happen.
pub struct LinpackExtended;

/// The config Quick Start's summary and the tab open on, when it exists.
pub const QUICK_PROFILE: &str = "11GB";
/// Always first on Quick Start: the package's own config.
pub const DEFAULT_PROFILE: &str = "Default";

/// One test's values, in the order config.json lists them.
pub const TEST_DEFAULTS: &[(&str, i64)] = &[
    ("minutes", 30),
    ("problem size", 38736),
    ("leading dimension", 38736),
    ("alignment value", 4),
];

pub const PROFILE_HINT: &str =
    "Saved as <name>.json in Linpack Extended's profiles folder, and shown as a button on Quick Start.";

pub fn test_defaults() -> Map<String, Value> {
    TEST_DEFAULTS
        .iter()
        .map(|(name, value)| (name.to_string(), Value::from(*value)))
        .collect()
}

fn as_object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

/// `int(test.get(name, default) or 0)`.
fn test_int(test: &Map<String, Value>, name: &str, default: i64) -> i64 {
    match test.get(name) {
        None => default,
        Some(value) if !truthy(value) => 0,
        Some(value) => py_int(value).unwrap_or(0),
    }
}

/// What one test's matrix takes, which is most of what it uses.
pub fn test_memory_gb(test: &Map<String, Value>) -> f64 {
    let size = test_int(test, "problem size", 0);
    let lda = match test_int(test, "leading dimension", 0) {
        0 => size,
        lda => lda,
    };
    8.0 * lda as f64 * size as f64 / 1024f64.powi(3)
}

/// One test with every value present and the leading dimension sane.
///
/// 0 means "work it out", and a leading dimension smaller than the problem
/// is one the benchmark cannot use, so both get Intel's figure.
pub fn clean_test(test: &Map<String, Value>) -> Map<String, Value> {
    let mut values: Vec<(String, i64)> = TEST_DEFAULTS
        .iter()
        .map(|(name, default)| (name.to_string(), test_int(test, name, *default)))
        .collect();
    values[0].1 = values[0].1.max(1);
    values[1].1 = values[1].1.max(1000);
    if values[2].1 < values[1].1 {
        values[2].1 = leading_dimension(values[1].1, true);
    }
    values
        .into_iter()
        .map(|(k, v)| (k, Value::from(v)))
        .collect()
}

/// The config.json these settings make.
pub fn config_json(config: &Config) -> Value {
    let mut chain: Vec<Map<String, Value>> = config
        .get("tests")
        .and_then(Value::as_array)
        .map(|tests| tests.iter().filter_map(as_object).map(clean_test).collect())
        .unwrap_or_default();
    if chain.is_empty() {
        chain.push(test_defaults());
    }
    let mut block = Map::new();
    block.insert(
        "reduce output below X problem size".into(),
        Value::from(cfg_int(config, "reduce_below", 0)),
    );
    block.insert(
        "track stats below X problem size".into(),
        Value::from(cfg_int(config, "track_below", 0)),
    );
    block.insert(
        "stop after residual mismatch".into(),
        Value::Bool(cfg_bool(config, "residual_check", false)),
    );
    // linpack.js reads this as `config.settings.KMP_AFFINITY ?? ""`, so
    // leaving the key out is the same as an empty one.
    let affinity = cfg_str(config, "affinity", "").trim().to_string();
    if !affinity.is_empty() {
        block.insert("KMP_AFFINITY".into(), Value::from(affinity));
    }
    let order: Vec<Value> = (1..=chain.len()).map(Value::from).collect();
    let tests: Map<String, Value> = chain
        .into_iter()
        .enumerate()
        .map(|(index, test)| ((index + 1).to_string(), Value::Object(test)))
        .collect();
    json!({
        "test order": order,
        "settings": block,
        "tests": tests,
    })
}

/// `json.dumps(value, indent=2)`: two-space indentation and non-ASCII
/// characters escaped, as Python writes them.
pub fn python_json(value: &Value) -> String {
    let text = serde_json::to_string_pretty(value).unwrap_or_default();
    ensure_ascii(&text)
}

/// Escape every non-ASCII character as `\uXXXX`. Safe on serialised JSON,
/// where such characters can only appear inside strings.
pub fn ensure_ascii(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii() {
            out.push(ch);
        } else {
            let mut units = [0u16; 2];
            for unit in ch.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

pub fn config_text(config: &Config) -> String {
    python_json(&config_json(config)) + "\n"
}

/// A config.json read back into the tab's settings.
pub fn profile_values_from(data: &Value) -> Result<Config, String> {
    let data = data.as_object().ok_or("the file is not a JSON object")?;
    let empty = Map::new();
    let block = data.get("settings").filter(|v| truthy(v));
    let block = match block {
        Some(value) => value.as_object().ok_or("its settings are not an object")?,
        None => &empty,
    };
    let tests = data.get("tests").filter(|v| truthy(v));
    let tests = match tests {
        Some(value) => value.as_object().ok_or("its tests are not an object")?,
        None => &empty,
    };
    let order: Vec<String> = match data.get("test order").filter(|v| truthy(v)) {
        Some(Value::Array(items)) => items.iter().map(py_str).collect(),
        Some(_) => return Err("its test order is not a list".into()),
        None => {
            let mut keys: Vec<String> = tests.keys().cloned().collect();
            keys.sort();
            keys
        }
    };
    let mut chain = Vec::new();
    for key in order {
        let Some(test) = tests.get(&key).filter(|t| truthy(t)) else {
            continue;
        };
        let test = test.as_object().ok_or("a test is not an object")?;
        let mut clean = Map::new();
        for (name, default) in TEST_DEFAULTS {
            let value = match test.get(*name) {
                None => *default,
                Some(value) => py_int(value).ok_or_else(|| format!("{name} is not a number"))?,
            };
            clean.insert(name.to_string(), Value::from(value));
        }
        chain.push(Value::Object(clean));
    }
    let affinity = match block.get("KMP_AFFINITY") {
        Some(value) if truthy(value) => py_str(value),
        _ => String::new(),
    };
    let block_int = |key: &str| -> i64 {
        match block.get(key) {
            Some(value) if truthy(value) => py_int(value).unwrap_or(0),
            _ => 0,
        }
    };
    Ok(config! {
        "tests" => chain,
        "residual_check" => block.get("stop after residual mismatch").map(truthy).unwrap_or(false),
        "affinity" => affinity,
        "reduce_below" => block_int("reduce output below X problem size"),
        "track_below" => block_int("track stats below X problem size"),
    })
}

impl LinpackExtended {
    pub fn package_dir(&self, root: &Path) -> Option<PathBuf> {
        // .../dependencies/linpack/linpack_xeon64.exe
        let exe = self.locate(root)?;
        Some(exe.parent()?.parent()?.parent()?.to_path_buf())
    }

    fn run_options() -> Config {
        config! {"threads" => hardware::logical_cores(), "show_window" => true}
    }
}

impl Profiles for LinpackExtended {
    /// (name, path) for each saved config, the package's Default first.
    fn profiles(&self, root: &Path) -> Vec<(String, PathBuf)> {
        let Some(package) = self.package_dir(root) else {
            return Vec::new();
        };
        let mut found: Vec<(String, PathBuf)> = glob::glob_in(&package, "profiles/*.json", false)
            .into_iter()
            .map(|path| {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (stem, path)
            })
            .collect();
        found.sort_by_key(|(name, _)| {
            let lower = name.to_lowercase();
            (lower != DEFAULT_PROFILE.to_lowercase(), lower)
        });
        found
    }

    fn profile_values(&self, path: &Path) -> Result<Config, String> {
        let text = fs::read(path).map_err(|e| e.to_string())?;
        let text = String::from_utf8(text).map_err(|e| e.to_string())?;
        let data: Value =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
        profile_values_from(&data)
    }

    fn save_profile(&self, root: &Path, name: &str, config: &Config) -> Result<PathBuf, ToolError> {
        let package = self.package_dir(root);
        let name = self.clean_profile_name(name);
        let (Some(package), false) = (package, name.is_empty()) else {
            return Err(unavailable("A config needs a name and Linpack Extended."));
        };
        let path = package.join("profiles").join(format!("{name}.json"));
        write_crlf(&path, &config_text(config))?;
        Ok(path)
    }

    fn profile_preview(&self, config: &Config) -> String {
        config_text(config).trim_end().to_string()
    }

    fn profile_hint(&self) -> &'static str {
        PROFILE_HINT
    }

    fn profile_preview_label(&self) -> &'static str {
        "config.json"
    }
}

impl Tool for LinpackExtended {
    fn key(&self) -> &'static str {
        "linpack_extended"
    }
    fn name(&self) -> &'static str {
        "Linpack Extended"
    }
    fn blurb(&self) -> &'static str {
        "The same Intel benchmark from the Linpack Extended package. Intel processors only -- \
         its binary refuses to run anywhere else. On AMD, use Linpack Xtreme instead."
    }
    fn exe_globs(&self) -> &'static [&'static str] {
        &["Linpack-Extended*/dependencies/linpack/linpack_xeon64.exe"]
    }
    fn console(&self) -> bool {
        true
    }
    fn detection_note(&self) -> &'static str {
        LINPACK_DETECTION_NOTE
    }

    // A config is a whole config.json -- the chain of tests and the settings
    // block -- kept as a file in the package's profiles folder, in exactly
    // the format linpack.js reads. The tests are not Fields: there can be any
    // number of them, so the tab draws them itself and they travel in the
    // config as "tests", a list in the order they run.
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::boolean("residual_check", "Stop on residual mismatch", false).hint(
                "\"stop after residual mismatch\". A residual that changes between identical \
                 trials is an error even when the check column still says pass.",
            ),
            Field::text("affinity", "KMP_AFFINITY", "").hint(
                "Blank leaves it out, and is the package's own answer to an OMP error at \
                 startup. It is also the only way the Threads setting below reaches the \
                 benchmark: when it is set, linpack.js replaces the environment with it.",
            ),
            Field::int("reduce_below", "Reduce output below", 0)
                .range(0, 200_000)
                .unit("n")
                .hint(
                    "\"reduce output below X problem size\". Quieter console for small tests. 0 \
                     is off.",
                ),
            Field::int("track_below", "Track stats below", 0)
                .range(0, 200_000)
                .unit("n")
                .hint(
                    "\"track stats below X problem size\". Min/Avg/Max GFlops for tests under \
                     this size. 0 is off.",
                ),
            Field::int("threads", "Threads", hardware::logical_cores())
                .range(1, 512)
                .hint("Not part of the config. Only used while KMP_AFFINITY is blank."),
            Field::boolean("show_window", "Show Linpack's window", true).hint(
                "Not part of the config. Its output is copied to a file at the same time, so \
                 nothing stops being checked.",
            ),
        ]
    }

    /// What the shared Linpack base class gave it. Its quick_config does not
    /// use it, so it only matters as "no preset".
    fn quick_start(&self) -> QuickStart {
        QuickStart {
            preset: Some("4 GB"),
            values: config! {"duration" => 30},
        }
    }

    fn has_apply_memory(&self) -> bool {
        true
    }
    fn apply_memory(&self, config: &mut Config) {
        apply_memory(config)
    }

    /// Why this build cannot run here, or "" when it can. Asked before
    /// anything is started, so a machine that cannot run it is told plainly
    /// instead of watching it exit cleanly having done nothing.
    fn unsupported_reason(&self, _root: &Path) -> String {
        if hardware::is_amd() {
            return "Linpack Extended ships the Intel-only build (linpack_xeon64.exe), which \
                    refuses to run on an AMD processor -- it prints \"runs on only genuine Intel \
                    processors\" and exits without testing anything. Use Linpack Xtreme on this \
                    machine; it ships an AMD build."
                .to_string();
        }
        String::new()
    }

    /// The 11GB config when it is there, otherwise the first one saved.
    fn quick_config(&self, root: &Path) -> Config {
        let mut config = self.defaults();
        config.insert("tests".into(), json!([Value::Object(test_defaults())]));
        let profiles = self.profiles(root);
        let chosen = profiles
            .iter()
            .rev()
            .find(|(name, _)| name == QUICK_PROFILE)
            .or_else(|| profiles.first());
        if let Some((_, path)) = chosen {
            if let Ok(values) = self.profile_values(path) {
                config = merged(&config, &values);
            }
        }
        merged(&config, &Self::run_options())
    }

    /// A button per saved config, the package's Default first.
    fn quick_actions(&self, root: &Path) -> Vec<(String, Config)> {
        let mut actions = Vec::new();
        for (name, path) in self.profiles(root) {
            let Ok(values) = self.profile_values(&path) else {
                continue;
            };
            actions.push((
                name,
                merged(&merged(&self.defaults(), &values), &Self::run_options()),
            ));
        }
        if actions.is_empty() {
            actions.push(("Start".into(), self.quick_config(root)));
        }
        actions
    }

    fn quick_summary(&self, root: &Path) -> String {
        let names: Vec<String> = self.profiles(root).into_iter().map(|(n, _)| n).collect();
        if names.is_empty() {
            return "No saved configs -- make one on the Linpack Extended tab.".into();
        }
        format!("Run {}", names.join(" / "))
    }

    fn as_profiles(&self) -> Option<&dyn Profiles> {
        Some(self)
    }

    fn is_test_chain(&self) -> bool {
        true
    }

    fn quick_profile_name(&self, root: &Path) -> String {
        let names: Vec<String> = self.profiles(root).into_iter().map(|(n, _)| n).collect();
        if names.iter().any(|n| n == QUICK_PROFILE) {
            return QUICK_PROFILE.to_string();
        }
        names.into_iter().next().unwrap_or_default()
    }

    /// Run the package the way the package runs itself.
    ///
    /// Linpack Extended is a Node driver around the same Intel binary, and it
    /// is worth using rather than going around: it writes the input file,
    /// chains tests, keeps Min/Avg/Max GFlops per problem size, and stops on
    /// a bad solve or a residual that moved -- printing "FAIL - severe
    /// instability detected" or "RESIDUAL MISMATCH - instability detected".
    ///
    /// Its settings live in config.json at the root of the package, and the
    /// path is hard-coded in linpack.js, so the chosen config is written
    /// there. The one that shipped is copied aside the first time rather than
    /// being written over and lost.
    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, ToolError> {
        let exe = self.locate(root).ok_or_else(|| {
            unavailable(format!(
                "{}'s binary was not found. Expected its folder beside this program.",
                self.name()
            ))
        })?;
        let blocked = self.unsupported_reason(root);
        if !blocked.is_empty() {
            return Err(unavailable(blocked));
        }

        // .../dependencies/linpack/linpack_xeon64.exe
        let dependencies = exe
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let package = dependencies
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let node = dependencies.join("node").join("node.exe");
        let driver = dependencies.join("linpack.js");
        for (needed, what) in [(&node, "node.exe"), (&driver, "linpack.js")] {
            if !needed.is_file() {
                return Err(unavailable(format!(
                    "Linpack Extended is incomplete: {what} is missing from its dependencies folder."
                )));
            }
        }

        let settings_path = package.join("config.json");
        let original = package.join("config.json.roch-original");
        if settings_path.exists() && !original.exists() {
            let _ = fs::copy(&settings_path, &original);
        }

        let document = config_json(config);
        write_crlf(&settings_path, &(python_json(&document) + "\n"))?;
        let chain: Vec<Map<String, Value>> = document["tests"]
            .as_object()
            .map(|tests| {
                tests
                    .values()
                    .filter_map(|t| t.as_object().cloned())
                    .collect()
            })
            .unwrap_or_default();

        let work = settings::run_dir("linpack");
        let log = work.join("linpack-extended-output.txt");
        remove_stale(&log);

        // What "Linpack Extended.bat" sets: node here is old enough that it
        // refuses to start on a recent Windows build without it. The thread
        // counts reach the benchmark only while KMP_AFFINITY is blank, because
        // that is the one case where linpack.js leaves the child's environment
        // alone.
        let threads = cfg_int(config, "threads", hardware::logical_cores());
        let env = vec![
            ("NODE_SKIP_PLATFORM_CHECK".to_string(), "1".to_string()),
            ("OMP_NUM_THREADS".to_string(), threads.to_string()),
            ("MKL_NUM_THREADS".to_string(), threads.to_string()),
        ];

        let tests: Vec<String> = chain
            .iter()
            .map(|t| {
                format!(
                    "n={} ({:.1} GB, {} min)",
                    test_int(t, "problem size", 0),
                    test_memory_gb(t),
                    test_int(t, "minutes", 0)
                )
            })
            .collect();
        let summary = format!("Linpack Extended {}, {} threads", tests.join(", "), threads);
        // linpack.js ends by itself once the last test's minutes are up and
        // says so; this is the countdown, with a little room for it to.
        let minutes: i64 = chain.iter().map(|t| test_int(t, "minutes", 0)).sum();
        let seconds = (minutes * 60 + 60).max(0) as u64;
        let complete = patterns(&["All tests successfully passed"]);

        let node_text = path_str(&node);
        let driver_text = path_str(&driver);
        if !cfg_bool(config, "show_window", true) {
            return Ok(LaunchSpec {
                argv: vec![node_text, driver_text],
                cwd: dependencies,
                env,
                console: true,
                error_key: self.key().into(),
                summary,
                duration_seconds: seconds,
                completion_patterns: complete,
                creation_flags: CREATE_NO_WINDOW,
                ..Default::default()
            });
        }

        // cmd, as the package's own .bat uses -- but cmd has no tee, and the
        // driver has no log option, so node splits it.
        let tee = work.join("tee.js");
        write_crlf(&tee, TEE_JS)?;
        let cmdline = format!(
            "cmd /c \"\"{node_text}\" \"{driver_text}\" 2>&1 | \"{node_text}\" \"{}\" \"{}\"\"",
            path_str(&tee),
            path_str(&log)
        );
        Ok(LaunchSpec {
            argv: vec![node_text, driver_text],
            cmdline: Some(cmdline),
            cwd: dependencies,
            env,
            console: false,
            watch_files: vec![log],
            error_key: self.key().into(),
            summary,
            duration_seconds: seconds,
            completion_patterns: complete,
            creation_flags: CREATE_NEW_CONSOLE,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intel_leading_dimension_rule() {
        assert_eq!(leading_dimension(38736, true), 38736);
        assert_eq!(leading_dimension(38720, true), 38736);
        assert_eq!(leading_dimension(1000, true), 1008);
        assert_eq!(leading_dimension(1024, true), 1040);
        assert_eq!(leading_dimension(1000, false), 1000);
        assert_eq!(leading_dimension(1008, false), 1016);
    }

    #[test]
    fn problem_sizes_fit_their_memory() {
        assert_eq!(problem_size_for(11448.0, true), 38736);
        for memory in [64.0, 2048.0, 4096.0, 6144.0, 8192.0, 14336.0, 30720.0] {
            let size = problem_size_for(memory, true);
            assert_eq!(size % 16, 0);
            assert!(8 * leading_dimension(size, true) * size <= memory as i64 * 1024 * 1024);
        }
        assert_eq!(problem_size_for(0.0, true), 16 * 22);
        assert_eq!(
            problem_size_for(0.1 * 1024.0, true),
            problem_size_for(102.0, true)
        );
    }

    #[test]
    fn config_json_matches_the_shipped_11gb_profile() {
        let config = config! {
            "tests" => json!([{"minutes": 30, "problem size": 38736, "leading dimension": 38736, "alignment value": 1}]),
            "residual_check" => true, "affinity" => "  ", "reduce_below" => 0, "track_below" => 0
        };
        let expected = "{\n  \"test order\": [\n    1\n  ],\n  \"settings\": {\n    \"reduce output below X problem size\": 0,\n    \"track stats below X problem size\": 0,\n    \"stop after residual mismatch\": true\n  },\n  \"tests\": {\n    \"1\": {\n      \"minutes\": 30,\n      \"problem size\": 38736,\n      \"leading dimension\": 38736,\n      \"alignment value\": 1\n    }\n  }\n}\n";
        assert_eq!(config_text(&config), expected);
    }

    #[test]
    fn tests_are_cleaned() {
        let test = json!({"minutes": 0, "problem size": 10, "leading dimension": 0});
        let clean = clean_test(test.as_object().unwrap());
        assert_eq!(
            Value::Object(clean),
            json!({"minutes": 1, "problem size": 1000, "leading dimension": 1008, "alignment value": 4})
        );
        let config = config! {"tests" => json!([]), "affinity" => "nowarnings,compact", "residual_check" => false};
        let document = config_json(&config);
        assert_eq!(document["tests"]["1"], Value::Object(test_defaults()));
        assert_eq!(
            document["settings"]["KMP_AFFINITY"],
            json!("nowarnings,compact")
        );
        assert_eq!(document["test order"], json!([1]));
        assert_eq!(ensure_ascii("a\u{e9}\u{1F600}"), "a\\u00e9\\ud83d\\ude00");
    }

    #[test]
    fn shipped_config_reads_back_in_its_own_order() {
        let data = json!({
            "test order": [2, 1],
            "settings": {
                "reduce output below X problem size": 0,
                "track stats below X problem size": 0,
                "stop after residual mismatch": false,
                "KMP_AFFINITY": "nowarnings,compact,1,0,granularity=fine"
            },
            "tests": {
                "1": {"minutes": 60, "problem size": 3536, "leading dimension": 3536, "alignment value": 4},
                "2": {"minutes": 60, "problem size": 39984, "leading dimension": 39984, "alignment value": 4}
            }
        });
        let values = profile_values_from(&data).unwrap();
        assert_eq!(values["tests"][0]["problem size"], json!(39984));
        assert_eq!(values["tests"][1]["problem size"], json!(3536));
        assert_eq!(
            values["affinity"],
            json!("nowarnings,compact,1,0,granularity=fine")
        );
        assert_eq!(values["residual_check"], json!(false));

        // Re-saving it renumbers in run order, as the Python version did.
        let document = config_json(&values);
        assert_eq!(document["tests"]["1"]["problem size"], json!(39984));
        assert_eq!(document["test order"], json!([1, 2]));

        // No order: the keys sorted as strings.
        let values =
            profile_values_from(&json!({"tests": {"2": {"minutes": 5}, "10": {"minutes": 7}}}))
                .unwrap();
        assert_eq!(values["tests"][0]["minutes"], json!(7));
        assert_eq!(values["tests"][1]["problem size"], json!(38736));
        assert!(profile_values_from(&json!([1])).is_err());
        assert!(profile_values_from(&json!({"tests": {"1": {"minutes": "x"}}})).is_err());
    }

    #[test]
    fn memory_per_test() {
        let test = json!({"problem size": 38736, "leading dimension": 0});
        let gb = test_memory_gb(test.as_object().unwrap());
        assert!((gb - 11.18).abs() < 0.01, "{gb}");
    }
}
