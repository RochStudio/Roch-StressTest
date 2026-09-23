# Roch StressTest

One window for every stress test on your PC. Roch StressTest starts Prime95, y-cruncher, TestMem5, RAM Test Pro, Linpack, OCCT, Cinebench, memtest_vulkan and 3DMark 11 with sensible defaults. It holds each test to a time limit and watches its output, so a failure stops the run as soon as it happens.

## Install

1. Download this repository (**Code → Download ZIP**) and unzip it to a folder you own.
2. Install [Python 3.13 (64-bit)](https://www.python.org/downloads/).
3. Run `RUN_AS_ADMIN.bat` and allow administrator access. The first run installs what it needs.

Cinebench and OCCT are too large to include. Download them separately and the app finds them the next time it starts:

- **Cinebench:** [maxon.net](https://www.maxon.net/en/downloads/cinebench-downloads). Install it, or unzip it next to the app.
- **OCCT:** [ocbase.com](https://www.ocbase.com/download). Put `OCCT.exe` in an `OCCT` folder next to the app.
- **3DMark 11:** found where it is installed.

To make a standalone EXE, run `BUILD_EXE.bat`.

## What it does

- **Quick Start:** a card for each tool. One click runs its defaults.
- **y-cruncher:** build your own profiles from the algorithms, memory, time per test and time limit. The command line is shown as you go, and each saved profile becomes a Quick Start button.
- **Linpack Extended:** build your own configs by chaining tests and choosing their size and length. Comes with **Default** (the package's own config) and **11GB**. Intel CPUs only.
- **TestMem5:** opens on the 1usmus v3 profile, set to run until you stop it.
- **memtest_vulkan:** GPU memory test for 10 min, 30 min or until stopped.
- **Prime95, RAM Test Pro, Linpack Xtreme, OCCT, Cinebench and 3DMark 11:** open in their own windows, still watched for errors.
- **Failure detection:** reads each tool's log or output and stops at the first error.
- **Extras:** a memory cleaner to free RAM before a run, and light/dark mode.

> Stress tests are meant to find instability. Watch temperatures, know your voltage limits, and don't leave a first run unattended.

## Credits

- **George Woltman / GIMPS — [Prime95](https://www.mersenne.org/download/)**
- **Alexander J. Yee — [y-cruncher](http://www.numberworld.org/y-cruncher/)**
- **Serj and CoolCmd — [TestMem5](https://github.com/CoolCmd/TestMem5)**, with the 1usmus and anta777 profiles
- **PCStonks — RAM Test Pro**
- **BoringBoredom — [Linpack Extended](https://github.com/BoringBoredom/Linpack-Extended)**
- **Regeneration — [Linpack Xtreme](https://www.ngohq.com/linpack-xtreme.html)**, both built on Intel's MKL Linpack benchmark
- **GpuZelenograd — [memtest_vulkan](https://github.com/GpuZelenograd/memtest_vulkan)**
- **OCBase — [OCCT](https://www.ocbase.com/)**, **Maxon — [Cinebench](https://www.maxon.net/en/cinebench)**, **UL — 3DMark 11**
- **Tom Schimansky — [CustomTkinter](https://github.com/TomSchimansky/CustomTkinter):** the interface toolkit.

Created by **Roch Studio / [@MateoPCTech](https://x.com/MateoPCTech)**. Licensed GPL-3.0; the bundled tools keep their own licenses.

[YouTube](https://www.youtube.com/@MateoPcTech) | [X](https://x.com/MateoPCTech) | [Discord](https://discord.gg/KfzExpKQHB)

[Detailed reference](docs/reference.md) · [License](LICENSE)
