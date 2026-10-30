//! Launch real bundled tools for a few seconds and stop them again.
//!
//! Ignored by default: they open windows on the desktop. Run them by hand
//! with `cargo test --test live -- --ignored --test-threads=1`. Only the
//! "open the menu" launches are used, so nothing is stress-tested.

use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use roch_stresstest::core::runner::{Event, RunState, Runner};
use roch_stresstest::tools;

fn running(image: &str) -> bool {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {image}"), "/NH"])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&output.stdout)
        .to_lowercase()
        .contains(&image.to_lowercase())
}

fn open_then_stop(key: &str, image: &str, label: &str) {
    let root = tools::tools_root();
    let tool = tools::by_key(key).unwrap();
    let (_, config) = tool
        .quick_actions(&root)
        .into_iter()
        .find(|(l, _)| l == label)
        .expect("an Open action");
    let spec = tool.build(&config, &root).expect("build");
    let mut runner = Runner::new();
    runner.start(spec, "live").expect("start");
    sleep(Duration::from_secs(4));
    assert!(runner.running(), "still supervised after four seconds");
    assert!(running(image), "{image} is running");
    runner.stop();
    runner.wait(Some(Duration::from_secs(30)));
    let states: Vec<RunState> = runner
        .drain()
        .into_iter()
        .filter_map(|e| match e {
            Event::State { state, .. } => Some(state),
            _ => None,
        })
        .collect();
    assert_eq!(states.last(), Some(&RunState::Stopped), "{states:?}");
    sleep(Duration::from_secs(1));
    assert!(!running(image), "{image} was killed with its tree");
}

#[test]
#[ignore]
fn ycruncher_menu_opens_and_is_killed() {
    open_then_stop("ycruncher", "y-cruncher.exe", "Open");
}

#[test]
#[ignore]
fn prime95_dialog_opens_and_is_killed() {
    open_then_stop("prime95", "prime95.exe", "Open");
}
