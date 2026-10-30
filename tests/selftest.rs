//! The self-test, run against a stand-in of the program folder.
//!
//! Every tool folder is recreated in a temporary directory with an empty file
//! where each executable would be, and the real profile files the repository
//! ships (y-cruncher's .bat files, Linpack Extended's configs) copied in, so
//! the whole of selftest.rs's walk -- every tab, every preset, every default
//! -- runs without touching the repository or the user's settings.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use roch_stresstest::app::panel::{PanelState, NEW_PROFILE};
use roch_stresstest::app::ui::StressApp;
use roch_stresstest::core::settings;
use roch_stresstest::selftest;
use roch_stresstest::tools::{self, linpack, prime95};
use serde_json::{json, Value};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"").unwrap();
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to.parent().unwrap()).unwrap();
    fs::copy(from, to).unwrap();
}

/// A fresh stand-in program folder, and a private settings folder.
fn fixture(name: &str) -> PathBuf {
    static DATA: OnceLock<PathBuf> = OnceLock::new();
    let data = DATA.get_or_init(|| {
        let data = std::env::temp_dir().join(format!("roch-selftest-data-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        data
    });
    settings::set_app_data_override(Some(data.clone()));

    let root = std::env::temp_dir().join(format!("roch-selftest-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let source = repo();
    touch(&root.join("p95v3019b20.win64/prime95.exe"));
    let yc = "y-cruncher v0.8.7.9547b";
    touch(&root.join(yc).join("y-cruncher.exe"));
    for bat in ["SFT.bat", "vt3.bat", "vt3 28gb.bat"] {
        copy(&source.join(yc).join(bat), &root.join(yc).join(bat));
    }
    touch(&root.join("TestMem5/TM5.exe"));
    touch(&root.join("TestMem5/bin/1usmus v3 @ 1usmus.cfg"));
    touch(&root.join("ram_test_pro_1.5.0/RAMTestPro1.5.0/RAM Test Pro.exe"));
    touch(&root.join("LinpackXtreme-1.1.7/LinpackXtreme_x64.exe"));
    let le = "Linpack-Extended-master";
    touch(
        &root
            .join(le)
            .join("dependencies/linpack/linpack_xeon64.exe"),
    );
    touch(&root.join(le).join("dependencies/node/node.exe"));
    touch(&root.join(le).join("dependencies/linpack.js"));
    copy(
        &source.join(le).join("config.json"),
        &root.join(le).join("config.json"),
    );
    for profile in ["Default.json", "11GB.json"] {
        copy(
            &source.join(le).join("profiles").join(profile),
            &root.join(le).join("profiles").join(profile),
        );
    }
    touch(&root.join("memtest_vulkan/memtest_vulkan-v0.5.0.exe"));
    touch(&root.join("OCCT/OCCT.exe"));
    touch(&root.join("CINEBENCH R23/Cinebench.exe"));
    touch(&root.join("3DMark 11/bin/x64/3DMark11.exe"));
    root
}

#[test]
fn every_launch_builds_and_every_default_agrees() {
    let root = fixture("walk");
    let mut out = Vec::new();
    let fails = selftest::run(&root, &mut out);
    let text = String::from_utf8(out).unwrap();
    println!("{text}");
    assert_eq!(fails, 0, "{text}");
    for tool in tools::tools() {
        assert!(
            text.contains(tool.name()),
            "{} missing from:\n{text}",
            tool.name()
        );
    }
    assert!(!text.contains("SKIP"));
    assert!(text.contains("memtest Vulkan Second GPU"));
    assert!(text.contains("failures: 0"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_missing_tool_is_skipped_not_failed() {
    let root = fixture("missing");
    fs::remove_dir_all(root.join("OCCT")).unwrap();
    let mut out = Vec::new();
    let fails = selftest::run(&root, &mut out);
    let text = String::from_utf8(out).unwrap();
    assert_eq!(fails, 0, "{text}");
    assert!(text.contains("SKIP OCCT (not available)"), "{text}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn quick_start_cards_and_tabs_come_up() {
    let root = fixture("app");
    let app = StressApp::headless(root.clone());
    let keys: Vec<&str> = app.panels().iter().map(|p| p.tool.key()).collect();
    assert_eq!(keys, ["ycruncher", "linpack_extended", "memtest_vulkan"]);
    let lines: Vec<&String> = app.log_lines().collect();
    assert!(lines[0].starts_with("Roch StressTest "));
    assert!(lines[1].contains("GB RAM"));

    // y-cruncher: an Open button and one per .bat, upper-cased.
    let yc = tools::by_key("ycruncher").unwrap();
    let labels: Vec<String> = yc
        .quick_actions(&root)
        .into_iter()
        .map(|(l, _)| l)
        .collect();
    // Sorted by file name, as Python's sorted(key=basename.lower()) did:
    // "vt3 28gb.bat" sorts before "vt3.bat" because a space precedes a dot.
    assert_eq!(labels, ["Open", "SFT", "VT3 28GB", "VT3"]);
    assert_eq!(
        yc.quick_summary(&root),
        "Open the menu, or run SFT / VT3 28GB / VT3"
    );

    // Linpack Extended: Default first, then the rest.
    let le = tools::by_key("linpack_extended").unwrap();
    let labels: Vec<String> = le
        .quick_actions(&root)
        .into_iter()
        .map(|(l, _)| l)
        .collect();
    assert_eq!(labels, ["Default", "11GB"]);
    assert_eq!(le.quick_summary(&root), "Run Default / 11GB");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ycruncher_launches_are_the_command_lines_the_python_built() {
    let root = fixture("yc");
    let yc = tools::by_key("ycruncher").unwrap();
    let exe = root.join("y-cruncher v0.8.7.9547b").join("y-cruncher.exe");

    let quick = yc.quick_config(&root);
    let spec = yc.build(&quick, &root).unwrap();
    let log = settings::run_dir("ycruncher").join("ycruncher.log");
    assert_eq!(
        spec.argv,
        [
            exe.display().to_string(),
            "pause:1".into(),
            "priority:0".into(),
            "skip-warnings".into(),
            format!("logfile:{}", log.display()),
            "stress".into(),
            "-M:28GB".into(),
            "-TL:1800".into(),
            "VSTv3".into(),
        ]
    );
    assert_eq!(spec.summary, "y-cruncher VSTv3, 28 GB, default test length");
    assert_eq!(spec.duration_seconds, 1800 + 300);
    assert!(spec.leave_open && !spec.console);
    assert_eq!(spec.watch_files, std::slice::from_ref(&log));

    // A .bat runs what it says, with logfile: and skip-warnings before stress.
    let (_, bat) = yc
        .quick_actions(&root)
        .into_iter()
        .find(|(label, _)| label == "VT3 28GB")
        .unwrap();
    let spec = yc.build(&bat, &root).unwrap();
    assert_eq!(
        spec.argv[1..],
        [
            "pause:1".to_string(),
            "priority:0".into(),
            "skip-warnings".into(),
            format!("logfile:{}", log.display()),
            "stress".into(),
            "-M:28GB".into(),
            "-D:60".into(),
            "-TL:216000".into(),
            "VSTv3".into(),
        ]
    );
    assert_eq!(spec.duration_seconds, 216_000 + 300);
    assert_eq!(
        spec.summary,
        "y-cruncher vt3 28gb.bat: pause:1 priority:0 stress -M:28GB -D:60 -TL:216000 VSTv3"
    );

    // Hidden: piped, no pause, pause:-2 first.
    let mut hidden = quick.clone();
    hidden.insert("show_window".into(), json!(false));
    hidden.insert("pause".into(), json!(false));
    let spec = yc.build(&hidden, &root).unwrap();
    assert!(spec.console && spec.watch_files.is_empty() && !spec.leave_open);
    // Startup parameters first, then what is added, then "stress".
    assert_eq!(
        spec.argv[1..4],
        [
            "priority:0".to_string(),
            "pause:-2".into(),
            "skip-warnings".into()
        ]
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ycruncher_profiles_save_load_and_delete() {
    let root = fixture("ycprofiles");
    let tool = tools::by_key("ycruncher").unwrap();
    let mut panel = PanelState::new(tool, &root);
    panel.profile.as_mut().unwrap().name = "  my: run?  ".into();
    let (name, existing) = panel.save_target();
    assert_eq!(name, "my run");
    assert!(existing.is_none());
    let path = panel.save_profile(&name).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "y-cruncher.exe pause:1 priority:0 stress -M:28GB -TL:1800 VSTv3\r\n"
    );
    assert_eq!(panel.profile.as_ref().unwrap().value(), "my run");
    assert!(panel.profile.as_ref().unwrap().delete_enabled());

    // Pick another, then pick it back: the boxes follow.
    panel.profile.as_mut().unwrap().selected = "vt3".into();
    panel.profile_picked().unwrap();
    assert_eq!(panel.config()["duration"], json!(30));
    assert_eq!(panel.config()["memory"], json!(0));
    assert_eq!(panel.label(), "y-cruncher -- vt3");
    panel.profile.as_mut().unwrap().selected = "my run".into();
    panel.profile_picked().unwrap();
    assert_eq!(panel.config()["memory"], json!(28));

    panel.delete_profile(&path).unwrap();
    assert!(!path.exists());
    assert_eq!(panel.profile.as_ref().unwrap().value(), NEW_PROFILE);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn linpack_extended_writes_its_config_and_keeps_the_original() {
    let root = fixture("linpack");
    let tool = tools::by_key("linpack_extended").unwrap();
    if !tool.unsupported_reason(&root).is_empty() {
        // AMD: refusing to build is the behaviour under test there.
        assert!(tool.build(&tool.quick_config(&root), &root).is_err());
        return;
    }
    let package = root.join("Linpack-Extended-master");
    let shipped = fs::read(package.join("config.json")).unwrap();
    let panel = PanelState::new(tool, &root);
    assert_eq!(panel.profile.as_ref().unwrap().value(), "11GB");
    let spec = tool.build(&panel.config(), &root).unwrap();

    assert_eq!(
        fs::read(package.join("config.json.roch-original")).unwrap(),
        shipped
    );
    let written = fs::read_to_string(package.join("config.json")).unwrap();
    assert!(
        written.contains("\r\n"),
        "written with CRLF, as Python's _write did"
    );
    let document: Value = serde_json::from_str(&written).unwrap();
    assert_eq!(document["tests"]["1"]["problem size"], json!(38736));
    assert_eq!(document["tests"]["1"]["alignment value"], json!(1));
    assert_eq!(
        document["settings"]["stop after residual mismatch"],
        json!(true)
    );

    let cmdline = spec.cmdline.clone().unwrap();
    assert!(cmdline.starts_with("cmd /c \"\""), "{cmdline}");
    assert!(cmdline.contains("2>&1 |"));
    assert!(
        cmdline.ends_with("linpack-extended-output.txt\"\""),
        "{cmdline}"
    );
    assert_eq!(spec.duration_seconds, 30 * 60 + 60);
    assert!(spec
        .summary
        .starts_with("Linpack Extended n=38736 (11.2 GB, 30 min), "));
    assert!(spec
        .env
        .iter()
        .any(|(k, v)| k == "NODE_SKIP_PLATFORM_CHECK" && v == "1"));
    assert_eq!(spec.completion_patterns, ["All tests successfully passed"]);
    let tee = settings::run_dir("linpack").join("tee.js");
    assert_eq!(
        fs::read_to_string(tee).unwrap(),
        linpack::TEE_JS.replace('\n', "\r\n")
    );

    // A second build leaves the saved original alone.
    tool.build(&panel.config(), &root).unwrap();
    assert_eq!(
        fs::read(package.join("config.json.roch-original")).unwrap(),
        shipped
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn windowed_tools_write_what_they_did() {
    let root = fixture("windowed");
    let p95 = tools::by_key("prime95").unwrap();
    let work = settings::run_dir("prime95");
    fs::write(work.join("local.txt"), "stale").unwrap();
    fs::write(work.join("results.txt"), "FATAL ERROR from last time").unwrap();
    let spec = p95.build(&p95.quick_config(&root), &root).unwrap();
    assert_eq!(
        fs::read_to_string(work.join("prime.txt")).unwrap(),
        prime95::PRIME_TXT.replace('\n', "\r\n")
    );
    assert!(!work.join("local.txt").exists() && !work.join("results.txt").exists());
    assert_eq!(spec.argv[1], format!("-W{}", work.display()));
    assert_eq!(p95.quick_summary(&root), "Opens its torture dialog");

    let tm5 = tools::by_key("testmem5").unwrap();
    fs::write(root.join("TestMem5/Log.txt"), "old").unwrap();
    let spec = tm5.build(&tm5.quick_config(&root), &root).unwrap();
    assert!(!root.join("TestMem5/Log.txt").exists());
    assert_eq!(
        fs::read_to_string(root.join("TestMem5/TM5.ini")).unwrap(),
        "[Settings]\r\nConfig File=1usmus v3 @ 1usmus.cfg\r\nMinutes=0\r\n"
    );
    assert!(spec.leave_open);
    assert_eq!(spec.completion_patterns.len(), 2);

    let ram = tools::by_key("ramtest").unwrap();
    let spec = ram.build(&ram.quick_config(&root), &root).unwrap();
    assert!(root
        .join("ram_test_pro_1.5.0/RAMTestPro1.5.0/logs")
        .is_dir());
    assert!(spec.watch_files[0].ends_with("logs/log.txt"));

    let mv = tools::by_key("memtest_vulkan").unwrap();
    let spec = mv.build(&mv.quick_config(&root), &root).unwrap();
    assert_eq!(spec.argv.len(), 2);
    assert_eq!(spec.argv[1], "1");
    assert_eq!(spec.duration_seconds, 15 * 60);
    assert_eq!(spec.summary, "memtest_vulkan GPU 1, auto size");
    let _ = fs::remove_dir_all(&root);
}
