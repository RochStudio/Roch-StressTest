# Roch StressTest

One window for every stress test on your PC. It starts each test with sensible defaults, holds it to a time limit, and stops the run at the first error.

## Install

1. Download `RochStressTest-<version>.zip` from [Releases](https://github.com/RochStudio/Roch-StressTest/releases).
2. Unzip it to a folder you own.
3. Run `RochStressTest.exe` and allow administrator access.

Windows 10/11 x64. Nothing else to install. Cinebench and OCCT are too large to include. Get them from [Maxon](https://www.maxon.net/en/downloads/cinebench-downloads) and [OCBase](https://www.ocbase.com/download), and put them next to the app.

## What it does

- **Quick Start:** one click runs any tool's defaults.
- **y-cruncher and Linpack Extended:** build and save your own profiles.
- **TestMem5, RAM Test Pro, Prime95, OCCT, Cinebench, memtest_vulkan, Linpack Xtreme, 3DMark 11:** launched and watched for errors.
- **Memory cleaner** and light/dark mode.

## Build from source

Roch StressTest is written in Rust. With [Rust](https://rustup.rs) installed (the `x86_64-pc-windows-msvc` toolchain):

```bat
BUILD_EXE.bat
```

That runs `cargo build --release` and copies `RochStressTest.exe` next to the tool folders, where it looks for them. `cargo test` runs the tests, and `RochStressTest.exe --selftest` builds every launch against the tools actually present. `--no-elevate` opens the window without the UAC prompt.

## Credits

- **George Woltman / GIMPS — [Prime95](https://www.mersenne.org/download/)**
- **Alexander J. Yee — [y-cruncher](http://www.numberworld.org/y-cruncher/)**
- **Serj and CoolCmd — [TestMem5](https://github.com/CoolCmd/TestMem5)**, with the 1usmus and anta777 profiles
- **PCStonks — RAM Test Pro**
- **BoringBoredom — [Linpack Extended](https://github.com/BoringBoredom/Linpack-Extended)**
- **Regeneration — [Linpack Xtreme](https://www.ngohq.com/linpack-xtreme.html)**
- **GpuZelenograd — [memtest_vulkan](https://github.com/GpuZelenograd/memtest_vulkan)**
- **OCBase — [OCCT](https://www.ocbase.com/)**, **Maxon — [Cinebench](https://www.maxon.net/en/cinebench)**, **UL — 3DMark 11**
- **Emil Ernerfeldt and contributors — [egui / eframe](https://github.com/emilk/egui)**, the window toolkit

Created by **Roch Studio / [@MateoPCTech](https://x.com/MateoPCTech)**. Licensed GPL-3.0; bundled tools keep their own licenses.

[YouTube](https://www.youtube.com/@MateoPcTech) | [X](https://x.com/MateoPCTech) | [Discord](https://discord.gg/KfzExpKQHB)
