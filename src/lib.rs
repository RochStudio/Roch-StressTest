//! Roch StressTest -- one window for every stress test on this machine.
//!
//! The program is a launcher and a supervisor. It does not implement a single
//! test: Prime95, y-cruncher, TestMem5, RAM Test Pro, both Linpacks, OCCT,
//! Cinebench, memtest_vulkan and 3DMark 11 all do their own work exactly as
//! they would if you started them yourself. What it adds is the part those
//! tools leave to you -- starting them with sensible defaults, holding them to
//! a time limit, and watching their output for a failure.
//!
//! ```text
//! core/     running a test and knowing what the machine is
//! app/      how it is drawn
//! tools/    one adapter per tool
//! selftest  builds every preset and checks the defaults agree
//! ```

pub mod app;
pub mod core;
pub mod selftest;
pub mod tools;
