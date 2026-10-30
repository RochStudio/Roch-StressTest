//! One tool's tab, as state: what is in each box, which profile and preset
//! are picked, and what pressing Start would run.
//!
//! Kept apart from the drawing (in `ui.rs`) so the self-test can build every
//! tab, pick every preset and read back the configuration exactly as the
//! window would, without opening a window -- which is what the Python
//! version's selftest.py did by driving a hidden Tk root.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::core::pyutil::split_ws;
use crate::core::toolbase::{py_str, truthy, Config, Field, FieldKind, Preset, Tool};
use crate::tools::linpack;

pub const NEW_PROFILE: &str = "New profile";

/// What one row's control holds.
#[derive(Debug, Clone, PartialEq)]
pub enum RowValue {
    /// An entry box: ints and free text alike, as typed.
    Text(String),
    Bool(bool),
    Choice(String),
    /// One tick per choice, in the field's order.
    Multi(Vec<bool>),
}

/// One editable setting: a label, a control, and an optional unit.
///
/// `value()` always returns something the tool's `build` will accept, so a
/// panel can read every field without checking any of them.
#[derive(Debug, Clone)]
pub struct FieldRow {
    pub field: Field,
    pub value: RowValue,
    pub enabled: bool,
}

impl FieldRow {
    pub fn new(field: Field) -> Self {
        let value = match field.kind {
            FieldKind::Multi => {
                let default = py_str(&field.default).replace(',', " ");
                let chosen = split_ws(&default);
                RowValue::Multi(
                    field
                        .choices
                        .iter()
                        .map(|c| chosen.contains(&c.as_str()))
                        .collect(),
                )
            }
            FieldKind::Bool => RowValue::Bool(truthy(&field.default)),
            FieldKind::Choice => {
                let default = py_str(&field.default);
                let default = if default.is_empty() {
                    field.choices.first().cloned().unwrap_or_default()
                } else {
                    default
                };
                RowValue::Choice(default)
            }
            FieldKind::Int | FieldKind::Text => RowValue::Text(py_str(&field.default)),
        };
        FieldRow {
            field,
            value,
            enabled: true,
        }
    }

    pub fn value(&self) -> Value {
        match &self.value {
            // In the order the tool lists them, not the order they were
            // ticked, so the same selection always makes the same command.
            RowValue::Multi(ticks) => Value::from(
                self.field
                    .choices
                    .iter()
                    .zip(ticks)
                    .filter(|(_, ticked)| **ticked)
                    .map(|(choice, _)| choice.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            RowValue::Bool(flag) => self.field.coerce(&Value::Bool(*flag)),
            RowValue::Choice(text) | RowValue::Text(text) => {
                self.field.coerce(&Value::from(text.clone()))
            }
        }
    }

    pub fn set(&mut self, value: &Value) {
        match &mut self.value {
            RowValue::Multi(ticks) => {
                let text = py_str(value).replace(',', " ");
                let chosen = split_ws(&text);
                for (tick, choice) in ticks.iter_mut().zip(&self.field.choices) {
                    *tick = chosen.contains(&choice.as_str());
                }
            }
            RowValue::Bool(flag) => *flag = truthy(value),
            RowValue::Choice(text) | RowValue::Text(text) => *text = py_str(value),
        }
    }
}

/// The profile picker: saved profiles, the one picked, and the save-as name.
#[derive(Debug, Clone)]
pub struct ProfileState {
    pub selected: String,
    pub name: String,
    pub profiles: Vec<(String, PathBuf)>,
}

impl ProfileState {
    pub fn names(&self) -> Vec<String> {
        let mut names = vec![NEW_PROFILE.to_string()];
        names.extend(self.profiles.iter().map(|(name, _)| name.clone()));
        names
    }

    pub fn path_of(&self, name: &str) -> Option<PathBuf> {
        self.profiles
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.clone())
    }

    /// What the picker reads: the selection when it is one of the names,
    /// otherwise "New profile", as the Tk option menu coerced it.
    pub fn value(&self) -> String {
        if self.names().contains(&self.selected) {
            self.selected.clone()
        } else {
            NEW_PROFILE.to_string()
        }
    }

    pub fn delete_enabled(&self) -> bool {
        self.path_of(&self.value()).is_some()
    }
}

/// The preset picker and the note under it.
#[derive(Debug, Clone)]
pub struct PresetState {
    pub selected: String,
    pub note: String,
}

/// The column order of a Linpack Extended test row, with headings.
pub const TEST_COLUMNS: [(&str, &str); 4] = [
    ("minutes", "Minutes"),
    ("problem size", "Problem size"),
    ("leading dimension", "Leading dim"),
    ("alignment value", "Alignment"),
];

/// Linpack Extended's chain of tests, as typed.
#[derive(Debug, Clone)]
pub struct TestsState {
    pub rows: Vec<[String; 4]>,
    pub add_gb: String,
}

impl TestsState {
    fn render(tests: &[Value]) -> Vec<[String; 4]> {
        tests
            .iter()
            .map(|test| {
                let empty = Map::new();
                let test = test.as_object().unwrap_or(&empty);
                TEST_COLUMNS.map(|(key, _)| test.get(key).map(py_str).unwrap_or_default())
            })
            .collect()
    }

    /// One row read back: every box an integer, 0 when it is not one.
    pub fn read(row: &[String; 4]) -> Map<String, Value> {
        TEST_COLUMNS
            .iter()
            .zip(row)
            .map(|((key, _), text)| {
                let value = text.trim().parse::<i64>().unwrap_or(0);
                (key.to_string(), Value::from(value))
            })
            .collect()
    }

    pub fn values(&self) -> Vec<Value> {
        self.rows
            .iter()
            .map(|row| Value::Object(Self::read(row)))
            .collect()
    }

    /// What one row's matrix takes, for the Memory column.
    pub fn memory_text(row: &[String; 4]) -> String {
        format!(
            "{:.1} GB",
            linpack::test_memory_gb(&linpack::clean_test(&Self::read(row)))
        )
    }

    /// Add a test sized for the gigabytes in the box.
    pub fn add(&mut self) {
        let gigabytes = self
            .add_gb
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|g| g.is_finite())
            .unwrap_or(11.0);
        let size = linpack::problem_size_for(gigabytes.max(0.1) * 1024.0, true);
        let mut test = linpack::test_defaults();
        test.insert("problem size".into(), Value::from(size));
        test.insert(
            "leading dimension".into(),
            Value::from(linpack::leading_dimension(size, true)),
        );
        self.rows.extend(Self::render(&[Value::Object(test)]));
    }

    pub fn remove(&mut self, index: usize) {
        if self.rows.len() > 1 && index < self.rows.len() {
            self.rows.remove(index);
        }
    }

    /// Move the row at *index* up one place.
    pub fn move_up(&mut self, index: usize) {
        if index > 0 && index < self.rows.len() {
            self.rows.swap(index - 1, index);
        }
    }
}

/// One tool's tab.
pub struct PanelState {
    pub tool: &'static dyn Tool,
    pub root: PathBuf,
    pub rows: Vec<FieldRow>,
    /// Values for fields that have no widget, because a preset sets them.
    pub hidden: Config,
    pub profile: Option<ProfileState>,
    pub preset: Option<PresetState>,
    pub tests: Option<TestsState>,
    /// Why the tool cannot run on this machine, or "".
    pub unsupported: String,
}

impl PanelState {
    pub fn new(tool: &'static dyn Tool, root: &Path) -> Self {
        let profile = tool.as_profiles().map(|profiles| ProfileState {
            selected: NEW_PROFILE.to_string(),
            name: String::new(),
            profiles: profiles.profiles(root),
        });
        let presets = tool.all_presets(root);
        let preset = presets.first().map(|first| PresetState {
            selected: first.name.clone(),
            note: String::new(),
        });
        let tests = tool.is_test_chain().then(|| TestsState {
            rows: Vec::new(),
            add_gb: "11".to_string(),
        });
        let rows = tool.fields().into_iter().map(FieldRow::new).collect();
        let mut panel = PanelState {
            tool,
            root: root.to_path_buf(),
            rows,
            hidden: Config::new(),
            profile,
            preset,
            tests,
            unsupported: tool.unsupported_reason(root),
        };
        panel.apply_quick_start();
        panel
    }

    pub fn presets(&self) -> Vec<Preset> {
        self.tool.all_presets(&self.root)
    }

    pub fn row_mut(&mut self, key: &str) -> Option<&mut FieldRow> {
        self.rows.iter_mut().find(|row| row.field.key == key)
    }

    fn set_value(&mut self, key: &str, value: &Value) {
        match self.row_mut(key) {
            Some(row) => row.set(value),
            None => {
                self.hidden.insert(key.to_string(), value.clone());
            }
        }
    }

    /// Put a profile's values into the boxes.
    pub fn load_values(&mut self, values: &Config) {
        if let (Some(tests), Some(list)) = (self.tests.as_mut(), values.get("tests")) {
            let list = match list.as_array() {
                Some(items) if !items.is_empty() => items.clone(),
                _ => vec![Value::Object(linpack::test_defaults())],
            };
            tests.rows = TestsState::render(&list);
        }
        for (key, value) in values {
            if let Some(row) = self.row_mut(key) {
                row.set(value);
            }
        }
    }

    /// Open this tab on the tool's default configuration: the same values
    /// the Quick Start page runs, so the two never disagree about what
    /// "default" means.
    pub fn apply_quick_start(&mut self) {
        if self.tests.is_some() {
            // Linpack Extended opens on the config Quick Start describes,
            // picked in the list.
            let name = self.tool.quick_profile_name(&self.root);
            if !name.is_empty() {
                if let Some(profile) = self.profile.as_mut() {
                    profile.selected = name.clone();
                    profile.name = name;
                }
            }
            let values = self.tool.quick_config(&self.root);
            self.load_values(&values);
            return;
        }
        let name = self.tool.quick_preset_name(&self.root);
        if self.preset.is_some() {
            if let Some(preset) = self.preset.as_mut() {
                if !name.is_empty() {
                    preset.selected = name.clone();
                }
            }
            self.apply_preset(&name, true);
        }
        for (key, value) in self.tool.quick_start().values {
            self.set_value(&key, &value);
        }
    }

    /// Pick a preset and fill the boxes it decides.
    pub fn apply_preset(&mut self, name: &str, initial: bool) {
        let Some(preset) = self.presets().into_iter().find(|p| p.name == name) else {
            return;
        };
        if let Some(state) = self.preset.as_mut() {
            state.note = preset.description.clone();
        }
        // A preset with no values of its own still decides which boxes it
        // leaves you free to fill in.
        let locked = self.tool.locked_fields(name);
        for row in self.rows.iter_mut() {
            row.enabled = !locked.contains(&row.field.key);
        }
        if preset.values.is_empty() && !initial {
            return;
        }
        for (key, value) in &preset.values {
            self.set_value(key, value);
        }
        // Memory is derived from the preset rather than stored in it, so that
        // the figure suits the machine it is running on today.
        if !preset.values.is_empty() && !locked.contains(&"memory") {
            if let Some(memory) = self.tool.suggested_memory(name) {
                if let Some(row) = self.row_mut("memory") {
                    row.set(&Value::from(memory));
                }
            }
        }
        if self.tool.has_apply_memory() && !preset.values.is_empty() {
            self.recompute_memory();
        }
    }

    /// A box was edited by hand.
    pub fn field_changed(&mut self, key: &str) {
        if key == "memory" && self.tool.has_apply_memory() {
            self.recompute_memory();
        }
    }

    /// Keep problem size and leading dimension in step with memory.
    fn recompute_memory(&mut self) {
        let mut config = self.config();
        self.tool.apply_memory(&mut config);
        for key in ["problem_size", "leading_dimension"] {
            if let Some(value) = config.get(key).cloned() {
                if let Some(row) = self.row_mut(key) {
                    row.set(&value);
                }
            }
        }
    }

    pub fn config(&self) -> Config {
        let mut values = self.hidden.clone();
        for row in &self.rows {
            values.insert(row.field.key.to_string(), row.value());
        }
        if let Some(tests) = &self.tests {
            values.insert("tests".into(), Value::Array(tests.values()));
        }
        values
    }

    /// What a run started from this tab is called in the log.
    pub fn label(&self) -> String {
        if let Some(profile) = &self.profile {
            let picked = profile.value();
            if picked != NEW_PROFILE {
                return format!("{} -- {}", self.tool.name(), picked);
            }
        }
        match &self.preset {
            None => self.tool.name().to_string(),
            Some(preset) => format!("{} -- {}", self.tool.name(), preset.selected),
        }
    }

    /// What saving would write, or "" for a tool without profiles.
    pub fn preview(&self) -> String {
        self.tool
            .as_profiles()
            .map(|p| p.profile_preview(&self.config()))
            .unwrap_or_default()
    }

    // -- profiles -----------------------------------------------------------

    pub fn rescan_profiles(&mut self) {
        let root = self.root.clone();
        if let (Some(profiles), Some(state)) = (self.tool.as_profiles(), self.profile.as_mut()) {
            state.profiles = profiles.profiles(&root);
        }
    }

    /// The picker changed. Loads the profile into the boxes; an error is the
    /// line to log when it could not be read.
    pub fn profile_picked(&mut self) -> Result<(), String> {
        self.rescan_profiles();
        let Some(state) = self.profile.as_mut() else {
            return Ok(());
        };
        let name = state.value();
        let Some(path) = state.path_of(&name) else {
            state.name.clear();
            return Ok(());
        };
        state.name = name;
        let Some(profiles) = self.tool.as_profiles() else {
            return Ok(());
        };
        let values = profiles
            .profile_values(&path)
            .map_err(|error| format!("Could not read {}: {}", path.display(), error))?;
        self.load_values(&values);
        Ok(())
    }

    /// Re-read the list and select *selected* when it is in it.
    pub fn refresh_profiles(&mut self, selected: &str) {
        self.rescan_profiles();
        if let Some(state) = self.profile.as_mut() {
            state.selected = if state.names().iter().any(|n| n == selected) {
                selected.to_string()
            } else {
                NEW_PROFILE.to_string()
            };
        }
    }

    /// The cleaned save-as name and the file it would replace, if any.
    pub fn save_target(&self) -> (String, Option<PathBuf>) {
        let (Some(profiles), Some(state)) = (self.tool.as_profiles(), self.profile.as_ref()) else {
            return (String::new(), None);
        };
        let name = profiles.clean_profile_name(&state.name);
        let existing = state.path_of(&name);
        (name, existing)
    }

    /// Write the boxes as profile *name*. Returns the path written.
    pub fn save_profile(&mut self, name: &str) -> Result<PathBuf, String> {
        let Some(profiles) = self.tool.as_profiles() else {
            return Err("this tool keeps no profiles".into());
        };
        let path = profiles
            .save_profile(&self.root, name, &self.config())
            .map_err(|e| e.to_string())?;
        if let Some(state) = self.profile.as_mut() {
            state.name = name.to_string();
        }
        self.refresh_profiles(name);
        Ok(path)
    }

    /// Delete the picked profile's file. Returns the path removed.
    pub fn delete_profile(&mut self, path: &Path) -> Result<(), String> {
        fs::remove_file(path).map_err(|e| e.to_string())?;
        if let Some(state) = self.profile.as_mut() {
            state.name.clear();
        }
        self.refresh_profiles(NEW_PROFILE);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools;

    #[test]
    fn rows_round_trip_values() {
        let mut row = FieldRow::new(Field::multi("a", "A", "VSTv3", &["BKT", "VSTv3"]));
        assert_eq!(row.value(), Value::from("VSTv3"));
        row.set(&Value::from("BKT,VSTv3"));
        assert_eq!(row.value(), Value::from("BKT VSTv3"));
        let mut row = FieldRow::new(Field::int("n", "N", 5).range(0, 10));
        row.value = RowValue::Text("50".into());
        assert_eq!(row.value(), Value::from(10));
        row.set(&Value::from(3));
        assert_eq!(row.value, RowValue::Text("3".into()));
    }

    #[test]
    fn tests_state_adds_moves_and_removes() {
        let mut tests = TestsState {
            rows: TestsState::render(&[Value::Object(linpack::test_defaults())]),
            add_gb: "2".into(),
        };
        tests.add();
        assert_eq!(tests.rows.len(), 2);
        assert_eq!(
            tests.rows[1][1],
            linpack::problem_size_for(2048.0, true).to_string()
        );
        tests.move_up(1);
        assert_eq!(
            tests.rows[0][1],
            linpack::problem_size_for(2048.0, true).to_string()
        );
        tests.remove(0);
        tests.remove(0);
        assert_eq!(tests.rows.len(), 1, "the last test cannot be removed");
        assert_eq!(TestsState::memory_text(&tests.rows[0]), "11.2 GB");
        tests.add_gb = "lots".into();
        tests.add();
        // Unreadable sizes fall back to 11 GB, as the Python tab did.
        assert_eq!(
            tests.rows[1][1],
            linpack::problem_size_for(11.0 * 1024.0, true).to_string()
        );
    }

    #[test]
    fn memtest_tab_opens_on_its_quick_start() {
        let tool = tools::by_key("memtest_vulkan").unwrap();
        let root = Path::new("Z:\\nowhere");
        let mut panel = PanelState::new(tool, root);
        assert_eq!(panel.preset.as_ref().unwrap().selected, "Standard");
        assert_eq!(panel.config()["duration"], 15);
        assert_eq!(panel.label(), "memtest Vulkan -- Standard");
        if let Some(preset) = panel.preset.as_mut() {
            preset.selected = "Second GPU".into();
        }
        panel.apply_preset("Second GPU", false);
        assert_eq!(panel.config()["device"], 2);
        assert_eq!(
            panel.preset.as_ref().unwrap().note,
            "For a machine with a second card, or an integrated one."
        );
        // Custom has no values: nothing changes.
        panel.apply_preset("Custom", false);
        assert_eq!(panel.config()["device"], 2);
    }

    #[test]
    fn ycruncher_tab_opens_on_its_quick_start() {
        let tool = tools::by_key("ycruncher").unwrap();
        let panel = PanelState::new(tool, Path::new("Z:\\nowhere"));
        assert_eq!(
            panel.preview(),
            "y-cruncher.exe pause:1 priority:0 stress -M:28GB -TL:1800 VSTv3"
        );
        assert_eq!(panel.label(), "y-cruncher");
        assert!(!panel.profile.as_ref().unwrap().delete_enabled());
    }
}
