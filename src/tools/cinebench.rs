//! Cinebench, in the versions worth keeping around.
//!
//! One tool with the version as its button rather than seven tabs, because
//! that is what the versions are: the same renderer, re-tuned.
//!
//! A word on what this is for. Cinebench is a benchmark: it reports a score,
//! not a verdict, and it has no idea whether the answer it computed was right.
//! It earns its place because a sustained all-core render is the load that
//! finds an unstable all-core clock or a cooling limit -- but a pass here
//! means "it finished", not "the arithmetic was correct". Prime95 and
//! y-cruncher are the tools that check their own answers.

use std::path::{Path, PathBuf};

use crate::config;
use crate::core::glob;
use crate::core::toolbase::{
    cfg_int, cfg_str, merged, path_str, unavailable, Config, Field, LaunchSpec, QuickStart, Tool,
    ToolError, CREATE_NO_WINDOW,
};

/// Where each version's executable is, oldest first, so the buttons read in
/// the order the versions came out. R11.5 is the same Cinema 4D-based build
/// as R15 and ships a 32-bit executable beside the 64-bit one; the 64-bit is
/// the one to run.
pub const VERSIONS: &[(&str, &[&str], &str, &str)] = &[
    (
        "R11.5",
        &["CINEBENCH R11.5"],
        "CINEBENCH Windows 64 Bit.exe",
        "old",
    ),
    (
        "R15",
        &["CINEBENCH R15"],
        "CINEBENCH Windows 64 Bit.exe",
        "old",
    ),
    (
        "R15 Extreme",
        &["CINEBENCH R15 EXTREME"],
        "CINEBENCH Windows 64 Bit.exe",
        "old",
    ),
    ("R20", &["CINEBENCH R20"], "Cinebench.exe", "new"),
    ("R23", &["CINEBENCH R23"], "Cinebench.exe", "new"),
    ("R24", &["CINEBENCH 2024"], "Cinebench.exe", "new"),
    ("R26", &["CINEBENCH 2026"], "Cinebench.exe", "new"),
];

/// Where an installed Cinebench turns up. A copy sitting beside this program
/// is preferred over any of these, and is the way to pin an exact build.
///
/// BenchMate's app folder is deliberately not searched: its copies are meant
/// to be launched by BenchMate with its own environment, and run straight out
/// of that folder they cannot resolve their own string resources -- Cinebench
/// 2024 comes up titled "StrNotFound".
pub const SEARCH_ROOTS: &[&str] = &[r"C:\Program Files\Maxon", r"C:\Program Files (x86)\Maxon"];

pub struct Cinebench;

fn newest(mut matches: Vec<PathBuf>) -> Option<PathBuf> {
    matches.retain(|m| m.is_file());
    matches.sort();
    matches.pop()
}

impl Cinebench {
    fn find(folders: &[&str], exe_name: &str) -> Option<PathBuf> {
        for root in SEARCH_ROOTS {
            let root = Path::new(root);
            for folder in folders {
                let pattern = format!("{folder}/*/{exe_name}");
                if let Some(found) = newest(glob::glob_in(root, &pattern, false)) {
                    return Some(found);
                }
                let pattern = format!("{folder}/{exe_name}");
                if let Some(found) = newest(glob::glob_in(root, &pattern, false)) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// Every Cinebench version present, as (label, path, generation).
    pub fn installed(&self, root: &Path) -> Vec<(&'static str, PathBuf, &'static str)> {
        let mut found = Vec::new();
        for (label, folders, exe_name, generation) in VERSIONS {
            // Beside this program first, so a copy dropped in the folder wins
            // over an installed one. Both the label and the real folder name
            // are tried: Maxon ships 2024 and 2026 in folders called
            // "Cinebench 2024" and "Cinebench 2026", neither of which
            // contains "R24" or "R26".
            let mut candidates: Vec<PathBuf> = Vec::new();
            let mut patterns = vec![format!("Cinebench*{label}")];
            patterns.extend(folders.iter().map(|f| f.to_string()));
            for pattern in patterns {
                candidates.extend(glob::glob_in(
                    root,
                    &format!("{pattern}/**/{exe_name}"),
                    true,
                ));
            }
            candidates.sort();
            candidates.dedup();
            let path = candidates.pop().or_else(|| Self::find(folders, exe_name));
            if let Some(path) = path {
                found.push((*label, path, *generation));
            }
        }
        found
    }
}

impl Tool for Cinebench {
    fn key(&self) -> &'static str {
        "cinebench"
    }
    fn name(&self) -> &'static str {
        "Cinebench"
    }
    fn blurb(&self) -> &'static str {
        "A sustained all-core render: the load that finds an unstable all-core clock or a \
         cooling limit. It reports a score rather than a verdict -- it does not check its own \
         arithmetic the way Prime95 does -- so a pass here means it finished, and a score that \
         drops between runs is worth as much as the number itself."
    }
    fn console(&self) -> bool {
        true
    }
    fn detection_note(&self) -> &'static str {
        "Cinebench has no notion of a wrong answer, so what is watched for is it failing to \
         run: a driver or renderer that falls over mid-render. The score is read off the 'CB' \
         line and reported when the run ends."
    }
    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0)
            .range(0, 100_000)
            .unit("min")
            .hint("0 lets it stay open for as many runs as you want.")]
    }
    // Configured in Cinebench's own window, so there is no tab here. Every
    // version installed gets a button that opens it.
    fn has_tab(&self) -> bool {
        false
    }
    fn quick_start(&self) -> QuickStart {
        QuickStart {
            preset: None,
            values: config! {"duration" => 0},
        }
    }

    fn locate(&self, root: &Path) -> Option<PathBuf> {
        self.installed(root)
            .into_iter()
            .next()
            .map(|(_, path, _)| path)
    }

    /// One button per installed version, each opening that Cinebench.
    fn quick_actions(&self, root: &Path) -> Vec<(String, Config)> {
        let found = self.installed(root);
        let base = self.quick_config(root);
        if found.is_empty() {
            return vec![("Start".into(), base)];
        }
        found
            .into_iter()
            .map(|(label, _, _)| {
                (
                    label.to_string(),
                    merged(&base, &config! {"version" => label}),
                )
            })
            .collect()
    }

    fn quick_summary(&self, root: &Path) -> String {
        let labels: Vec<&str> = self
            .installed(root)
            .into_iter()
            .map(|(l, _, _)| l)
            .collect();
        if labels.is_empty() {
            return "None found".into();
        }
        format!("Opens {}", labels.join(" / "))
    }

    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, ToolError> {
        let found = self.installed(root);
        if found.is_empty() {
            return Err(unavailable(
                "No Cinebench installation was found. Expected a folder beside this program, or \
                 an installed copy.",
            ));
        }
        let mut wanted = cfg_str(config, "version", "");
        if !found.iter().any(|(label, _, _)| *label == wanted) {
            let mut labels: Vec<&str> = found.iter().map(|(l, _, _)| *l).collect();
            labels.sort();
            wanted = labels[0].to_string();
        }
        let exe = found
            .iter()
            .find(|(label, _, _)| *label == wanted)
            .map(|(_, path, _)| path.clone())
            .unwrap_or_default();

        // Opened, not driven: which test a version offers differs between R15
        // and R26, so the run is started in the window. stdout is still read
        // -- Cinebench prints its result there even when the run was started
        // by hand -- but there is no completion pattern, because treating the
        // first score as the end of the run would close it under somebody in
        // the middle of a second.
        Ok(LaunchSpec {
            argv: vec![path_str(&exe)],
            cwd: exe.parent().map(Path::to_path_buf).unwrap_or_default(),
            console: true,
            error_key: self.key().into(),
            summary: format!("Cinebench {wanted} (run it in its own window)"),
            duration_seconds: cfg_int(config, "duration", 0).max(0) as u64 * 60,
            leave_open: true,
            creation_flags: CREATE_NO_WINDOW,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_copies_are_found_by_label_or_folder_name() {
        let dir = std::env::temp_dir().join(format!("roch-cb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Cinebench 2024/app")).unwrap();
        std::fs::create_dir_all(dir.join("CINEBENCH R23")).unwrap();
        std::fs::write(dir.join("Cinebench 2024/app/Cinebench.exe"), b"").unwrap();
        std::fs::write(dir.join("CINEBENCH R23/Cinebench.exe"), b"").unwrap();
        let found = Cinebench.installed(&dir);
        let local: Vec<&str> = found
            .iter()
            .filter(|(_, path, _)| path.starts_with(&dir))
            .map(|(label, _, _)| *label)
            .collect();
        assert_eq!(local, ["R23", "R24"]);
        let actions = Cinebench.quick_actions(&dir);
        assert!(actions
            .iter()
            .any(|(label, config)| label == "R24" && config["version"] == "R24"));
        let spec = Cinebench
            .build(&config! {"version" => "R23"}, &dir)
            .unwrap();
        assert!(spec.argv[0].ends_with("Cinebench.exe"));
        assert!(spec.summary.starts_with("Cinebench R23"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
