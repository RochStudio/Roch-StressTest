//! 3DMark 11, opened as a graphics load.
//!
//! Its window is where everything about a 3DMark run is decided: which
//! benchmark, how many loops, which adapter. There is a command-line runner
//! beside it, `bin\x64\3DMark11Cmd.exe`, which can set all of that from
//! outside -- but it needs the Professional edition, and Advanced (which the
//! free legacy key gives) unlocks looping in the window instead. So this
//! opens the window and lets it be driven there.
//!
//! Worth being clear about: a looping benchmark reports nothing when the
//! machine is merely wrong. It catches a driver reset, a hang, or a crash --
//! which is how an unstable GPU usually fails -- but it does not verify a
//! single pixel. memtest_vulkan is the one that checks its answers.

use std::path::Path;

use crate::config;
use crate::core::toolbase::{
    cfg_int, limit_summary, path_str, unavailable, Config, Field, LaunchSpec, QuickStart, Tool,
    ToolError,
};

pub const SEARCH_ROOTS: &[&str] = &[
    r"C:\Program Files\Futuremark\3DMark 11",
    r"C:\Program Files (x86)\Futuremark\3DMark 11",
    r"C:\Program Files\Futuremark\3DMark11",
];

pub struct ThreeDMark11;

impl Tool for ThreeDMark11 {
    fn key(&self) -> &'static str {
        "3dmark11"
    }
    fn name(&self) -> &'static str {
        "3DMark 11"
    }
    fn blurb(&self) -> &'static str {
        "A looping graphics load: shaders, geometry and the whole board at a game-like duty \
         cycle, which is how an unstable GPU core clock usually gets caught. It checks nothing, \
         so what it finds is a driver reset or a hang rather than a wrong answer. Opens in its \
         own window, where Advanced can set a loop count; the command-line runner that does it \
         unattended needs the Professional edition."
    }
    // The application itself, not 3DMarkLauncher.exe beside it. The launcher
    // is a stub: it starts this and exits immediately, so a run supervised
    // through it reported "finished with no errors" a second after starting.
    fn exe_globs(&self) -> &'static [&'static str] {
        &["3DMark 11*/bin/x64/3DMark11.exe"]
    }
    fn external_globs(&self) -> Vec<String> {
        SEARCH_ROOTS
            .iter()
            .map(|root| format!(r"{root}\bin\x64\3DMark11.exe"))
            .collect()
    }
    fn console(&self) -> bool {
        true
    }
    fn detection_note(&self) -> &'static str {
        "A looping benchmark has no right answer to check, so what stops the run is a failed \
         workload, a removed or hung device, or the process dying -- the shapes an unstable \
         graphics card actually fails in. Run from its own window there is no output to read, \
         so the time limit and a crash are what this program can see."
    }
    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0)
            .range(0, 100_000)
            .unit("min")
            .hint("0 lets the loop you set in 3DMark's own window run until you press Stop.")]
    }
    // Configured in 3DMark's own window, so there is no tab here.
    fn has_tab(&self) -> bool {
        false
    }
    fn quick_start(&self) -> QuickStart {
        QuickStart {
            preset: None,
            values: config! {"duration" => 0},
        }
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
                "3DMark 11 was not found. Expected bin\\x64\\3DMark11.exe under Program \
                 Files\\Futuremark\\3DMark 11.",
            )
        })?;
        // The window, and only the window. The runner still holds this to a
        // time limit and still notices it dying.
        Ok(LaunchSpec {
            argv: vec![path_str(&exe)],
            cwd: exe.parent().map(Path::to_path_buf).unwrap_or_default(),
            console: false,
            error_key: self.key().into(),
            summary: "3DMark 11 (preset and loop set in its own window)".into(),
            duration_seconds: cfg_int(config, "duration", 0).max(0) as u64 * 60,
            leave_open: true,
            ..Default::default()
        })
    }
}
