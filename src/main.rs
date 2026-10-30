//! Roch StressTest -- one window for every stress test on this machine.
//!
//! ```text
//! RochStressTest.exe               asks for administrator rights, then opens
//! RochStressTest.exe --no-elevate  opens without asking (for working on the UI)
//! RochStressTest.exe --selftest    builds every launch and prints the results
//! ```

// A window, not a console, in a release build -- the same as running it with
// pyw rather than py. A debug build keeps its console for diagnostics.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case)]

use roch_stresstest::app::{platform, ui};
use roch_stresstest::{selftest, tools};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--selftest") {
        platform::attach_parent_console();
        let mut out = std::io::stdout();
        let fails = selftest::run(&tools::tools_root(), &mut out);
        std::process::exit(if fails > 0 { 1 } else { 0 });
    }

    // TestMem5 and RAM Test Pro need administrator rights to lock physical
    // pages, and Prime95 needs them to set affinity. Asking once at launch
    // beats each tool failing differently three hours into a queue.
    //
    // --no-elevate is for looking at the window without a UAC prompt. The
    // tools themselves still need administrator rights, so a run started this
    // way will fail in the tool rather than here.
    let admin = platform::is_admin();
    if !args.iter().any(|a| a == "--no-elevate") && !admin {
        eprintln!("Admin privileges required. Relaunching with UAC prompt...");
        platform::relaunch_as_admin();
        std::process::exit(0);
    }

    if let Err(error) = ui::run(admin) {
        eprintln!("Could not open the window: {error}");
        std::process::exit(1);
    }
}
