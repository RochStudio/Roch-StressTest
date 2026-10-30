//! A port of the parts of Python's `glob` module the tool adapters use.
//!
//! The semantics are Python's on Windows, not a shell's: matching is
//! case-insensitive, `*` and `?` never cross a path separator, names starting
//! with a dot are only matched by a pattern that starts with one, and `**`
//! means "any number of directories" only when `recursive` is asked for --
//! otherwise it is an ordinary `*`. That last point matters: RAM Test Pro is
//! found with `ram_test_pro*/**/RAM Test Pro.exe`, which Python reads as one
//! directory level, not a walk of the whole tree.

use std::fs;
use std::path::{Path, PathBuf};

fn has_magic(text: &str) -> bool {
    text.contains(['*', '?', '['])
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// `fnmatch.fnmatch` with Windows case folding.
pub fn fnmatch(name: &str, pattern: &str) -> bool {
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    match_from(&name, &pattern)
}

/// Parse a `[...]` set starting at `pattern[start]` (the `[`). Returns the
/// index just past the closing `]` and whether *ch* is in the set, or None
/// when there is no closing bracket (then `[` is a literal, as in Python).
fn match_class(pattern: &[char], start: usize, ch: char) -> Option<(usize, bool)> {
    let mut index = start + 1;
    let mut negate = false;
    if index < pattern.len() && pattern[index] == '!' {
        negate = true;
        index += 1;
    }
    // A ']' straight after the opening bracket is part of the set.
    let first = index;
    let mut found = false;
    while index < pattern.len() && (pattern[index] != ']' || index == first) {
        let low = pattern[index];
        if index + 2 < pattern.len() && pattern[index + 1] == '-' && pattern[index + 2] != ']' {
            let high = pattern[index + 2];
            if low <= ch && ch <= high {
                found = true;
            }
            index += 3;
        } else {
            if low == ch {
                found = true;
            }
            index += 1;
        }
    }
    if index >= pattern.len() {
        return None;
    }
    Some((index + 1, found != negate))
}

fn match_from(name: &[char], pattern: &[char]) -> bool {
    let (mut n, mut p) = (0usize, 0usize);
    // Where to resume after the last '*', for backtracking.
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() {
            match pattern[p] {
                '*' => {
                    star = Some((p, n));
                    p += 1;
                    continue;
                }
                '?' => {
                    n += 1;
                    p += 1;
                    continue;
                }
                '[' => {
                    if let Some((next, hit)) = match_class(pattern, p, name[n]) {
                        if hit {
                            n += 1;
                            p = next;
                            continue;
                        }
                    } else if name[n] == '[' {
                        n += 1;
                        p += 1;
                        continue;
                    }
                }
                literal => {
                    if literal == name[n] {
                        n += 1;
                        p += 1;
                        continue;
                    }
                }
            }
        }
        match star {
            Some((star_p, star_n)) => {
                p = star_p + 1;
                n = star_n + 1;
                star = Some((star_p, star_n + 1));
            }
            None => return false,
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

fn entries(dir: &Path) -> Vec<(String, PathBuf, bool)> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(|entry| entry.ok())
        .map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            // is_dir() on the path follows links, as Python's does.
            let is_dir = path.is_dir();
            (name, path, is_dir)
        })
        .collect()
}

/// Every directory under *dir* (not *dir* itself), skipping hidden ones.
fn walk_dirs(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 64 {
        return;
    }
    for (name, path, is_dir) in entries(dir) {
        if is_dir && !is_hidden(&name) {
            out.push(path.clone());
            walk_dirs(&path, out, depth + 1);
        }
    }
}

/// Match *pattern* (relative, `/` or `\` separated) under *root*. *root* is
/// taken literally, so a folder name with brackets in it is not misread.
pub fn glob_in(root: &Path, pattern: &str, recursive: bool) -> Vec<PathBuf> {
    let parts: Vec<&str> = pattern
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return if root.exists() {
            vec![root.to_path_buf()]
        } else {
            Vec::new()
        };
    }
    let mut current = vec![root.to_path_buf()];
    for (index, part) in parts.iter().enumerate() {
        let last = index + 1 == parts.len();
        let mut next = Vec::new();
        for base in &current {
            if recursive && *part == "**" {
                // Zero or more directories. In the final position Python also
                // yields the files; nothing here uses that, but it is cheap.
                next.push(base.clone());
                let mut dirs = Vec::new();
                walk_dirs(base, &mut dirs, 0);
                next.extend(dirs);
                if last {
                    for (name, path, is_dir) in entries(base) {
                        if !is_dir && !is_hidden(&name) {
                            next.push(path);
                        }
                    }
                }
            } else if !has_magic(part) {
                let candidate = base.join(part);
                let keep = if last {
                    candidate.exists()
                } else {
                    candidate.is_dir()
                };
                if keep {
                    next.push(candidate);
                }
            } else {
                for (name, path, is_dir) in entries(base) {
                    if is_hidden(&name) && !part.starts_with('.') {
                        continue;
                    }
                    if !last && !is_dir {
                        continue;
                    }
                    if fnmatch(&name, part) {
                        next.push(path);
                    }
                }
            }
        }
        // "**/x/**" can reach the same directory twice; Python dedups the
        // same way when it is asked to.
        let mut seen = std::collections::HashSet::new();
        next.retain(|path| seen.insert(path.clone()));
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current
}

/// Match an absolute pattern such as `C:\Program Files\OCCT\OCCT*.exe`.
pub fn glob(pattern: &str, recursive: bool) -> Vec<PathBuf> {
    let parts: Vec<&str> = pattern.split(['/', '\\']).collect();
    let first_magic = parts.iter().position(|part| has_magic(part));
    let Some(split) = first_magic else {
        let path = PathBuf::from(pattern);
        return if path.exists() {
            vec![path]
        } else {
            Vec::new()
        };
    };
    let mut prefix = parts[..split].join("\\");
    if prefix.ends_with(':') || prefix.is_empty() {
        prefix.push('\\');
    }
    let rest = parts[split..].join("/");
    glob_in(Path::new(&prefix), &rest, recursive)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnmatch_like_python() {
        assert!(fnmatch("p95v3019b20.win64", "p95*"));
        assert!(fnmatch("PRIME95.EXE", "prime95.exe"));
        assert!(fnmatch("y-cruncher v0.8.7.9547b", "y-cruncher*"));
        assert!(!fnmatch("y-cruncher v0.8.7", "y-cruncher?"));
        assert!(fnmatch("abc", "a?c"));
        assert!(fnmatch("a1", "a[0-9]"));
        assert!(!fnmatch("ax", "a[0-9]"));
        assert!(fnmatch("ax", "a[!0-9]"));
        assert!(fnmatch("a[", "a["));
        assert!(fnmatch("", "*"));
        assert!(fnmatch("OCCT.exe", "OCCT*.exe"));
        assert!(fnmatch("memtest_vulkan-v0.5.0.exe", "memtest_vulkan*.exe"));
        assert!(!fnmatch("memtest_vulkan.log", "memtest_vulkan*.exe"));
        assert!(fnmatch("aXbXc", "*X*X*"));
    }

    #[test]
    fn globbing_a_tree() {
        let dir = std::env::temp_dir().join(format!("roch-glob-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("ram_test_pro_1.5.0/RAMTestPro1.5.0/deep/er")).unwrap();
        fs::create_dir_all(dir.join(".hidden")).unwrap();
        fs::write(
            dir.join("ram_test_pro_1.5.0/RAMTestPro1.5.0/RAM Test Pro.exe"),
            b"",
        )
        .unwrap();
        fs::write(
            dir.join("ram_test_pro_1.5.0/RAMTestPro1.5.0/deep/er/RAM Test Pro.exe"),
            b"",
        )
        .unwrap();
        fs::write(dir.join(".hidden/RAM Test Pro.exe"), b"").unwrap();

        // Non-recursive: ** is one level, like Python.
        let found = glob_in(&dir, "ram_test_pro*/**/RAM Test Pro.exe", false);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("RAMTestPro1.5.0/RAM Test Pro.exe"));

        // Recursive: every depth, including zero, but never hidden folders.
        let found = glob_in(&dir, "**/RAM Test Pro.exe", true);
        assert_eq!(found.len(), 2, "{found:?}");

        // A literal pattern is an existence check.
        assert_eq!(glob_in(&dir, "ram_test_pro_1.5.0", false).len(), 1);
        assert!(glob_in(&dir, "nothing/here.exe", false).is_empty());

        let absolute = format!("{}\\ram_test_pro*\\*\\RAM*.exe", dir.display());
        assert_eq!(glob(&absolute, false).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
