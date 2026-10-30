//! The registry: every tool this program knows how to start, in tab order.
//!
//! Order is deliberate. The two CPU tests come first because they are what
//! you reach for after a clock change, then the two memory tests, then the two
//! Linpacks -- the harshest, and the ones most likely to be run last once
//! everything else has passed. Cinebench follows as a load rather than a
//! check, and the two graphics tools come last because they test a different
//! piece of hardware entirely.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::core::toolbase::Tool;

pub mod cinebench;
pub mod linpack;
pub mod memtest_vulkan;
pub mod occt;
pub mod prime95;
pub mod ramtest;
pub mod testmem5;
pub mod threedmark11;
pub mod ycruncher;

/// Every tool, in tab order.
pub fn tools() -> &'static [Box<dyn Tool>] {
    static TOOLS: OnceLock<Vec<Box<dyn Tool>>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        vec![
            Box::new(prime95::Prime95),
            Box::new(ycruncher::YCruncher),
            Box::new(testmem5::TestMem5),
            Box::new(ramtest::RamTestPro),
            Box::new(linpack::LinpackXtreme),
            Box::new(linpack::LinpackExtended),
            // After the Linpacks because it is the same kind of thing done
            // from one window -- CPU, memory and the two together, each
            // checking its answers.
            Box::new(occt::Occt),
            Box::new(cinebench::Cinebench),
            Box::new(memtest_vulkan::MemtestVulkan),
            Box::new(threedmark11::ThreeDMark11),
        ]
    })
}

pub fn by_key(key: &str) -> Option<&'static dyn Tool> {
    tools()
        .iter()
        .find(|tool| tool.key() == key)
        .map(|b| b.as_ref())
}

/// Quick Start's two columns, written out rather than split down the middle
/// of the tab order, so related tools sit together and stay where they were
/// put. Anything in `tools()` and not named here still appears: it goes to
/// whichever column is shorter, so a new tool is never simply invisible.
pub const QUICK_COLUMNS: [&[&str]; 2] = [
    &[
        "cinebench",
        "prime95",
        "linpack_extended",
        "linpack_xtreme",
        "occt",
    ],
    &[
        "ycruncher",
        "testmem5",
        "ramtest",
        "3dmark11",
        "memtest_vulkan",
    ],
];

/// The tools for each Quick Start column, in the order they are shown.
pub fn quick_columns() -> [Vec<&'static dyn Tool>; 2] {
    let mut columns: [Vec<&'static dyn Tool>; 2] = [
        QUICK_COLUMNS[0]
            .iter()
            .filter_map(|key| by_key(key))
            .collect(),
        QUICK_COLUMNS[1]
            .iter()
            .filter_map(|key| by_key(key))
            .collect(),
    ];
    for tool in tools() {
        let placed = columns.iter().flatten().any(|t| t.key() == tool.key());
        if !placed {
            let side = if columns[0].len() <= columns[1].len() {
                0
            } else {
                1
            };
            columns[side].push(tool.as_ref());
        }
    }
    columns
}

/// The folder the tool distributions are unpacked into: beside the
/// executable. A build run straight out of `target\release` or
/// `target\debug` is treated as living in the source folder, two or three
/// levels up, so `cargo run` finds the tools the repository ships.
pub fn tools_root() -> PathBuf {
    let exe = std::env::current_exe().ok();
    let beside = exe
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    for ancestor in beside.ancestors() {
        let name = ancestor
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase());
        if name.as_deref() == Some("target") {
            if let Some(project) = ancestor.parent() {
                if project.join("Cargo.toml").is_file() {
                    return project.to_path_buf();
                }
            }
        }
    }
    beside
}

/// The tools whose executables are actually present.
pub fn available(root: &Path) -> Vec<&'static dyn Tool> {
    tools()
        .iter()
        .map(|t| t.as_ref())
        .filter(|t| t.available(root))
        .collect()
}

pub fn missing(root: &Path) -> Vec<&'static dyn Tool> {
    tools()
        .iter()
        .map(|t| t.as_ref())
        .filter(|t| !t.available(root))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_registered_once_and_placed() {
        let keys: Vec<&str> = tools().iter().map(|t| t.key()).collect();
        assert_eq!(keys.len(), 10);
        let mut unique = keys.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), keys.len());
        let columns = quick_columns();
        assert_eq!(columns[0].len() + columns[1].len(), keys.len());
        assert_eq!(columns[0][0].key(), "cinebench");
        assert_eq!(columns[1][0].key(), "ycruncher");
    }

    #[test]
    fn tab_order_matches_the_original() {
        let keys: Vec<&str> = tools().iter().map(|t| t.key()).collect();
        assert_eq!(
            keys,
            [
                "prime95",
                "ycruncher",
                "testmem5",
                "ramtest",
                "linpack_xtreme",
                "linpack_extended",
                "occt",
                "cinebench",
                "memtest_vulkan",
                "3dmark11"
            ]
        );
        let tabbed: Vec<&str> = tools()
            .iter()
            .filter(|t| t.has_tab())
            .map(|t| t.key())
            .collect();
        assert_eq!(tabbed, ["ycruncher", "linpack_extended", "memtest_vulkan"]);
    }
}
