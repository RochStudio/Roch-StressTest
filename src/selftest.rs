//! Build every tab and every preset, and check each produces a real launch.
//!
//! A launcher fails in a way unit tests miss: a preset that names a field the
//! tool does not have, or a discovered profile that has since been renamed,
//! breaks only when somebody opens that tab and presses Start. This walks
//! every combination the window can produce and builds the launch spec for
//! each, so that failure happens here instead.
//!
//! Nothing is executed -- the specs are built and thrown away. Building them
//! does write what pressing Start would write (Prime95's prime.txt, TM5.ini's
//! defaults, Linpack Extended's config.json), exactly as selftest.py did.
//!
//! ```text
//! RochStressTest.exe --selftest
//! ```

use std::io::Write;
use std::path::Path;

use serde_json::Value;

use crate::app::panel::PanelState;
use crate::core::toolbase::{py_int, Tool};
use crate::tools;

fn short(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Run the checks against the tools under *root*, writing one line per
/// check to *out*. Returns the number of failures.
pub fn run(root: &Path, out: &mut dyn Write) -> usize {
    let mut fails = 0;
    let mut panels: Vec<PanelState> = tools::tools()
        .iter()
        .map(|t| t.as_ref())
        .filter(|t| t.has_tab() && t.available(root))
        .map(|t| PanelState::new(t, root))
        .collect();

    for tool in tools::tools().iter().map(|t| t.as_ref()) {
        let panel = panels.iter_mut().find(|p| p.tool.key() == tool.key());
        let Some(panel) = panel else {
            if !tool.available(root) {
                let _ = writeln!(out, "SKIP {} (not available)", tool.name());
                continue;
            }
            // No tab, because the tool is configured in its own window. The
            // default still has to build -- that is what Quick Start runs.
            match tool.build(&tool.quick_config(root), root) {
                Ok(spec) => {
                    let _ = writeln!(out, "OK   {:<13} no tab: {}", tool.name(), spec.summary);
                }
                Err(error) => {
                    fails += 1;
                    let _ = writeln!(out, "FAIL {:<13} no tab, default: {}", tool.name(), error);
                }
            }
            continue;
        };

        let blocked = tool.unsupported_reason(root);
        if !blocked.is_empty() {
            // A tool that cannot run here must refuse to build, not build
            // something that will exit having done nothing.
            match tool.build(&tool.quick_config(root), root) {
                Err(error) => {
                    let _ = writeln!(
                        out,
                        "OK   {:<17} refuses to run here: {}",
                        tool.name(),
                        short(&error.to_string(), 52)
                    );
                }
                Ok(_) => {
                    fails += 1;
                    let _ = writeln!(
                        out,
                        "FAIL {:<17} is blocked but still built a launch spec",
                        tool.name()
                    );
                }
            }
            continue;
        }

        let presets = panel.presets();
        if presets.is_empty() {
            let config = panel.config();
            match tool.build(&config, root) {
                Ok(spec)
                    if spec
                        .argv
                        .first()
                        .map(|a| Path::new(a).is_file())
                        .unwrap_or(false) =>
                {
                    let _ = writeln!(
                        out,
                        "OK   {:<17} {:<24} {}",
                        tool.name(),
                        "(no presets)",
                        short(&spec.summary, 52)
                    );
                }
                Ok(_) => {
                    fails += 1;
                    let _ = writeln!(
                        out,
                        "FAIL {:<17} {:<24} exe missing",
                        tool.name(),
                        "(no presets)"
                    );
                }
                Err(error) => {
                    fails += 1;
                    let _ = writeln!(
                        out,
                        "FAIL {:<17} {:<24} {}",
                        tool.name(),
                        "(no presets)",
                        error
                    );
                }
            }
        }

        for preset in presets {
            if let Some(state) = panel.preset.as_mut() {
                state.selected = preset.name.clone();
            }
            panel.apply_preset(&preset.name, false);
            let config = panel.config();
            match tool.build(&config, root) {
                Ok(spec) => {
                    let exe_ok = spec
                        .argv
                        .first()
                        .map(|a| Path::new(a).is_file())
                        .unwrap_or(false);
                    let cwd_ok = spec.cwd.is_dir();
                    if exe_ok && cwd_ok {
                        let _ = writeln!(
                            out,
                            "OK   {:<13} {:<28} {}",
                            tool.name(),
                            preset.name,
                            short(&spec.summary, 52)
                        );
                    } else {
                        fails += 1;
                        let _ = writeln!(
                            out,
                            "FAIL {:<13} {:<28} exe {exe_ok} cwd {cwd_ok}",
                            tool.name(),
                            preset.name
                        );
                    }
                }
                Err(error) => {
                    fails += 1;
                    let _ = writeln!(
                        out,
                        "FAIL {:<13} {:<28} {}",
                        tool.name(),
                        preset.name,
                        error
                    );
                }
            }
        }
    }

    // The Quick Start page and each tool's own tab must agree on the default,
    // because they are advertised as the same thing.
    let _ = writeln!(out);
    for tool in tools::tools().iter().map(|t| t.as_ref()) {
        let Some(panel) = panels.iter_mut().find(|p| p.tool.key() == tool.key()) else {
            continue;
        };
        if !tool.unsupported_reason(root).is_empty() {
            continue;
        }
        panel.apply_quick_start();
        let quick = tool.quick_config(root);
        let tab = panel.config();
        let differs = disagreements(tool, root, &quick, &tab);
        if differs.is_empty() {
            let _ = writeln!(
                out,
                "OK   {:<13} default: {}",
                tool.name(),
                tool.quick_summary(root)
            );
        } else {
            fails += 1;
            let _ = writeln!(
                out,
                "FAIL {:<13} tab disagrees with Quick Start: {}",
                tool.name(),
                differs.join(", ")
            );
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "failures: {fails}");
    fails
}

/// The keys on which the Quick Start default and the tab's opening values
/// differ, ignoring what is meant to differ.
pub fn disagreements(
    tool: &dyn Tool,
    root: &Path,
    quick: &serde_json::Map<String, Value>,
    tab: &serde_json::Map<String, Value>,
) -> Vec<String> {
    // Values derived from free memory are recomputed each time they are
    // asked for, so the two reads can land a megabyte or two apart.
    let derived: &[&str] = if tool.suggested_memory("").is_some() || tool.has_apply_memory() {
        &["memory", "problem_size", "leading_dimension"]
    } else {
        &[]
    };
    // A field the preset locks is filled in by the tool itself, so whatever
    // is left in the box never reaches the run.
    let locked = tool.locked_fields(&tool.quick_preset_name(root));
    let apart = |key: &str, left: &Value, right: &Value| -> bool {
        if !derived.contains(&key) {
            return left != right;
        }
        match (py_int(left), py_int(right)) {
            (Some(l), Some(r)) => (l - r).abs() as f64 > 8f64.max(l as f64 * 0.01),
            _ => left != right,
        }
    };
    quick
        .iter()
        .filter(|(key, _)| !locked.contains(&key.as_str()))
        .filter_map(|(key, left)| {
            let right = tab.get(key)?;
            apart(key, left, right).then(|| format!("{key}: {left} vs {right}"))
        })
        .collect()
}
