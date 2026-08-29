# Roch StressTest

**Roch StressTest** — one window for every stress test on the machine. Prime95, y-cruncher, TestMem5, RAM Test Pro, both Linpacks, OCCT, Cinebench, memtest_vulkan and 3DMark 11, each with presets that say what they actually do, a time limit, and live failure detection. Python 3.13 / CustomTkinter, same theme as [Roch Viewer](https://github.com/RochStudio/Roch-Viewer) and [Roch GPU OC](https://github.com/RochStudio/Roch-GPU).

It does not implement a single test. Every tool runs exactly as it would if you started it yourself; what this adds is the part they leave to you.

```
Roch StressTest/
|- main.py                 the window: Quick Start, tool cards, log
|- selftest.py             builds every preset and checks the defaults agree
|
|- core/                   running a test and knowing what the machine is
|   |- runner.py           starts a test, watches it, enforces the time limit
|   |- errors.py           the patterns that decide a run has failed
|   |- toolbase.py         what every tool adapter is made of
|   |- hardware.py         cores, RAM and CPU vendor, for the defaults
|   |- memory.py           the memory cleaner and the live RAM readout
|   |- settings.py         where runs, logs and settings are kept
|   |- winui.py            finding a running tool's windows
|   \- version.py          the product name and version, written once
|
|- app/                    how it is drawn
|   |- theme.py            the shared Roch palette
|   \- widgets.py          the field/section widgets the cards are built from
|
|- tools/                  one adapter per tool
|   |- __init__.py         the registry, and the Quick Start columns
|   \- prime95.py, ycruncher.py, testmem5.py, ramtest.py, linpack.py,
|                          occt.py, cinebench.py, memtest_vulkan.py,
|                          threedmark11.py
|
\- <tool folders>          Prime95, y-cruncher, TestMem5, RAM Test Pro,
                           both Linpacks, memtest_vulkan. Cinebench and OCCT
                           go here too but are not committed -- they are
                           gigabytes, and several files are past GitHub's
                           100 MB limit.
```

## Run it

```bat
RUN_AS_ADMIN.bat
```

Or build a standalone EXE with `BUILD_EXE.bat` and copy `dist\RochStressTest.exe` up one level, next to the tool folders — it looks for them beside itself.

Administrator rights are asked for once at launch. TestMem5 and RAM Test Pro need them to lock physical pages, Prime95 to set affinity. Asking once beats a tool failing three hours into a run for want of a privilege.

## What each tool does

**Quick Start** is the front page and, for half the tools, the only page: two columns of cards, one per tool. One test runs at a time.

A tool gets a tab of its own only when this program has settings worth offering. Prime95, Linpack Xtreme, OCCT, Cinebench and 3DMark 11 do not: each decides something that cannot be known from here -- Prime95 derives its FFT ranges from the cache it finds, Linpack Xtreme's menu picks the build matching the processor, 3DMark's looping needs a licence most installations lack. Their cards have an **Open** button and nothing else, and the run is set up in the window that owns those settings. A tab of fields that are then discarded would be worse than no tab. Where a tool does have a tab, it opens on the same defaults as its card, so the two never disagree about what "default" means.

| Tool | Presets | Driven by |
|---|---|---|
| **Prime95** | none -- chosen in Prime95's own dialog | `prime95 -W<dir>`, with no `-t`, so it opens its torture dialog and waits |
| **y-cruncher** | no presets — all eight algorithms as tick boxes, plus a button per `.bat` beside it | `y-cruncher … stress -M -D -TL <algorithms>` |
| **TestMem5** | none -- the profile is picked in TM5's own window | opens `TM5.exe` with no arguments |
| **RAM Test Pro** | none -- set in its own window | opens `RAM Test Pro.exe` |
| **Linpack Xtreme** | none -- answered at its own menu | opens `LinpackXtreme_x64.exe` |
| **Linpack Extended** | 2 / 4 / 6 / 8 / 11 / 14 / 30 GB, Intel CPUs only | its own Node driver, in cmd: `config.json` written from the fields, then `node linpack.js` |
| **OCCT** | none -- chosen in OCCT's own window | opens `OCCT.exe` |
| **Cinebench** | none -- one button per version installed (R11.5 through R26) | opens that version's executable |
| **memtest Vulkan** | first GPU / second GPU | the device index as a bare argument |
| **3DMark 11** | none -- chosen in 3DMark's own window | opens `bin\x64\3DMark11.exe` |

y-cruncher has no presets: its algorithms are not alternatives, so all eight are tick boxes and any combination runs. Ticking none runs the lot, which is y-cruncher's own convention. Its card also carries a button for every `.bat` sitting beside `y-cruncher.exe`, read from the folder rather than listed here — drop one in and it gets a button, named after the file. Those run exactly what the file says, with `logfile:` and `skip-warnings` added and nothing taken away.

Cinebench gets a button per version installed, R11.5 through R26, each opening that version. It is looked for beside this program and under `Program Files\Maxon`. BenchMate's copies are deliberately *not* used: they are meant to be launched by BenchMate with its own environment, and run directly they cannot resolve their string resources -- Cinebench 2024 comes up titled `StrNotFound`, with a licence agreement whose Accept and Decline buttons are both labelled `StrNotFound` as well.

Not everything here checks its own answers, and the difference matters. Prime95, y-cruncher, Linpack, TestMem5, RAM Test Pro, OCCT and memtest_vulkan all verify what they computed and can tell you the machine is *wrong*. Cinebench and 3DMark 11 do not: they are benchmarks, loading the hardware hard and reporting a score, but a pass means "it finished", not "the arithmetic was right". Use them for heat, sustained clocks and driver stability; use the others for correctness.

Memory figures are computed from what is actually free when you pick a preset, not from a constant. Linpack's problem size and leading dimension are derived from the memory box using Intel's own rule — the nearest odd multiple of 16 at or above the problem size on AVX parts.

### Linpack Extended runs its own driver

Both packages are front-ends around the same Intel binary, and both are now used rather than gone around. Linpack Xtreme's is a console menu that asks for memory, trials and time and picks the right build for the processor itself, so it is opened and answered there. Linpack Extended's is a Node script, and it is the thing its settings are documented against. The fields on the tab are written to `config.json` in the package, in that project's own format, and `node linpack.js` is started in a cmd window. The `config.json` that shipped is copied to `config.json.roch-original` the first time rather than being written over and lost.

Using it means its failure detection rather than ours: it parses every result row itself and prints `FAIL - severe instability detected`, or `RESIDUAL MISMATCH - instability detected` when a residual moves between identical trials. It also chains tests and tracks Min/Avg/Max GFlops per problem size, which the raw binary does not.

Two consequences worth knowing. cmd has no `tee`, and the driver has no log option, so its output would be either on screen or readable and never both -- the package already ships `node.exe`, so a nine-line Node script splits it to the console and to a file the runner reads.

And KMP_AFFINITY decides whether the Threads field means anything, which is why it defaults to blank here. `linpack.js` overrides the child's environment *only* when KMP_AFFINITY is set -- and when it does, it replaces the environment rather than adding to it, so `OMP_NUM_THREADS` and `MKL_NUM_THREADS` never arrive and the default `compact,1,0,granularity=fine` placement gives one thread per physical core: 8 of 16 on an 8C/16T part, half the load the tab asked for. Left blank, the binary inherits the environment and runs the threads it was told to. Blank is also this package's own documented answer to an OMP error at startup.

## Memory cleaner

The toolbar carries a live `RAM x / y MB free` readout and a **Clean memory** button, because a memory test only tests the memory it can actually get. Windows counts its standby list — pages it has finished with but is holding in case they are wanted again — as used, so on a machine that has been up a while, asking for 28 GB quietly takes part of it from the page file and the run measures an SSD rather than the DIMMs.

Cleaning empties every process's working set, then purges the standby and low-priority standby lists, through `NtSetSystemInformation` with `SystemMemoryListInformation`. That needs `SeProfileSingleProcessPrivilege`, which an administrator has but which is *not* enabled in the token until it is asked for. It is implemented in `memory.py` rather than bundling a third-party binary; the approach was checked against danskee's Memory Cleaner, whose DLL exports one `CleanMemory` and imports exactly those three calls.

Nothing about it is destructive: the purged pages are cached copies of data already on disk, re-read if wanted again.

## Defaults

What each tool runs from Quick Start, and what its tab opens on where it has one:

| Tool | Default |
|---|---|
| Prime95 | opens its torture dialog, no time limit |
| y-cruncher | VSTv3 ticked, 28 GB, 30 min |
| TestMem5 | opens its window, no time limit |
| RAM Test Pro | opens its window, no time limit |
| Linpack Xtreme | opens its menu, no time limit |
| Linpack Extended | 11 GB (problem size 38736), 30 min, residual checks on, alignment 1, KMP_AFFINITY blank (Intel only) |
| OCCT | opens its window, no time limit |
| Cinebench | one button per version installed, each opens it |
| memtest Vulkan | first GPU, 15 min |
| 3DMark 11 | opens its window, no time limit |

TM5's cycle count lives inside the `.cfg`, so overriding it means writing a copy. That copy goes to `bin/Roch active.cfg`, with only the `Cycles=` line changed and every other byte identical. The stock profiles are never edited: a cycle count set here is a property of the run, not of the profile.

Every default is reproduced as the command line you would have typed. y-cruncher's, for instance, is `pause:1 stress -M:28GB -TL:1800 VSTv3` — plus `skip-warnings`, without which it waits at a startup prompt nobody is there to answer, and `logfile:` for an artifact you can open afterwards -- which is also what lets a run be watched while staying visible in its own console. `colors:0` is deliberately not passed: it was, on the assumption that colour would put escape codes in the log, and it does not. A log written with colour on contains no `0x1b` byte at all, so all that setting did was take the colour off the window you are watching.

## Failure detection

The point of the program. What each tab claims is what it does:

- **y-cruncher** and **Linpack** are read live from the process. y-cruncher prints `Running VSTv3: Passed` once per iteration and the negative of that line stops the run within a second.
- **Linpack** additionally has every result row parsed. Linpack does *not* stop on a bad solve — it writes something other than `pass` in the eighth column and carries on — so that column is checked, and so is the residual, which must not drift between identical trials.
- **Prime95** is watched through `results.txt`, which it writes on every error. A worker that fails while you are away is still caught.
- **TestMem5** is read from `Log.txt`, which 0.13.1 appends beside `TM5.exe`. `Error in test #N`, a non-zero error total, and a crash blamed on the tested memory all stop the run. TM5's *own* failures — memory it could not allocate or lock — are deliberately not reported as instability, because TM5 says outright that they are not.
- **RAM Test Pro** is read from `logs/log.txt`: a non-zero `Test errors detected` and its own `ERROR` lines. `ERROR! Free up RAM…` is *it* failing rather than the memory, and is reported as a setup problem. Its settings are entered in its own window: it has no command line, no usable settings file and no registry keys, and the four boxes that matter exist nowhere else.

Every tool runs in its own visible window. The three that are windowed do that by themselves; the two console ones are given a console, which is why neither is piped: a child whose stdout is redirected leaves its own console blank. y-cruncher is watched through its `logfile:` instead, and Linpack — which has no log option, and whose pass/fail column exists nowhere but its output — is teed with PowerShell's `Tee-Object`. That writes UTF-16, so the log reader detects encoding from the byte-order mark. Turn the window off per tool with "Show ...'s window" and it goes back to a hidden console read straight off the pipe.

A tool that finishes on its own is left on screen rather than killed, so y-cruncher's `pause:1` and TM5's summary are actually readable — killing the process the moment the verdict was known is what made `pause:1` look broken. Anything still running (a time limit, a stop, a failure) is stopped properly. Because y-cruncher checks `-TL` only between tests and routinely overruns it, the runner's own clock allows five minutes past that limit before stepping in.

An exit is judged on what the tool said, not just on its exit code. Windows gives a console process exit code `0xC000013A` when it is sent Ctrl+C or its window is closed; that says nothing about the machine, so it is reported as stopped, never as a failure. A clean exit from a tool that announces its own completion, *without* that announcement, is also stopped rather than passed — closing a window mid-run is not a pass. A real failure still wins over both: the error line is checked first.

A failure kills the process tree, beeps, switches to the Log tab, and writes a transcript to `%LOCALAPPDATA%\RochStressTest\logs\FAILED-<timestamp>.txt` — because the run that matters is the one that failed at 4am.

## Five things found while building this

All are handled in the code; they are written down because each cost an afternoon and none is documented anywhere obvious.

**Prime95 torture settings do not live in `local.txt`.** Every reference, including Prime95's own `undoc.txt`, says they do. In 30.19 that is a migration path: give it a `local.txt` and it folds those keys into `prime.txt`, deletes `local.txt`, and carries on. `tool_prime95.py` writes `prime.txt` directly -- these days only the four keys that suppress its startup prompts, but the same applies to anything put there. Verifiable — ask for `MinTortureFFT=MaxTortureFFT=1024` and `results.txt` says `Self-test 1024K passed!` and nothing else.

**Linpack needs `MKL_DEBUG_CPU_TYPE=5` on Zen 5, and the two packages are not interchangeable.** Without it `linpack_amd64.exe` dies with an illegal instruction before printing a single result row — its kernel dispatch picks a path the CPU does not implement. The documented `MKL_ENABLE_INSTRUCTIONS=AVX2` does not fix it; only this does. The Intel-branded builds (`linpack_intel64.exe`, and Linpack Extended's `linpack_xeon64.exe`) refuse to run on AMD at all: they print "runs on only genuine Intel processors" and exit with status **zero**, so nothing is tested and nothing looks wrong. Linpack Xtreme ships an AMD build and is the one to use there; Linpack Extended is a separate tool that refuses to start on AMD rather than pretending to run.

**Prime95's four torture presets are not fixed ranges, and its dialog cannot be preset from a file.** Two separate things, and together they decide how a launcher has to drive it.

Every guide quotes Large FFTs as 2048K–8192K. In 30.19 it is worked out from the caches of the processor it finds: on a 14900KS with 36 MB of L3 and its E-cores off, Large FFTs starts at **957K**, and a launcher that writes 2048K skips the band from 957K to 2048K entirely. That band is not academic — 960K is exactly where that chip returned `FATAL ERROR: Rounding was 0.4990028871`. Hard-coding any range means running the right test on the machine it was read off and a different one everywhere else.

The dialog cannot be steered any other way, because it does not read `prime.txt` at all. Seventeen variants were tried — every FFT range from 4 to 4096, with and without a memory figure, and a `prime.txt` Prime95 itself had written with Large FFTs selected. All seventeen opened on **Blend**, with Blend's own figures in the boxes. It also resets to Blend when reopened *mid-run*, while the Large FFTs test it was used to start is still going. So the radio button can never confirm what a launcher configured: read `MinTortureFFT`, or the `FFT length 960K` in the worker lines, and ignore the button entirely.

Both of those are why Prime95 has no tab here. A tab of FFT sizes and memory figures could not be shown to agree with what Prime95 was going to do -- the numbers differ per machine, and the dialog contradicts them on sight. Quick Start opens Prime95 with that dialog up and lets it be answered in the one place that decides it. `results.txt` is still watched, so a worker that fails at 4am is still caught.

**TestMem5 does write a log.** It is generally taken as read that TM5's error count exists only on its own window. In 0.13.1 it also appends to `Log.txt` beside `TM5.exe` — the configuration it loaded, the memory it took, every error, and how the run ended — so a TM5 failure at 3am is catchable after all. The patterns in `errors.TESTMEM5` are TM5's own message templates, read out of the strings in `TM5.exe` and `TM5.dll` rather than guessed, including the ones that must *not* count: TM5 separates a failure of the memory under test from a failure of TM5 itself (`Failed to allocate memory for testing`, `WARNING! Failed to lock memory`) and says outright, "This is not a failure of tested memory."

**TestMem5 0.13.1 does not take a `.cfg` path, and does not exit when it finishes.** Two separate traps, both fatal to a launcher.

Every guide for the older TM5 says to pass the configuration as the first argument. 0.13.1 answers that with *"Something is wrong on the command line"* in a message box, then sits there testing nothing — so the launcher looks like it worked. The real usage string exists only inside `TM5.dll`:

```
TM5.exe Config File="CoolCmd @ CoolCmd.cfg" Minutes=180
```

A bare file name resolved inside `bin\` (a `bin\` prefix or an absolute path both fail), and a parameter name with a space in it, which is why this is the one tool launched from a raw command line rather than an argument list.

Then, having finished its cycles, TM5 writes `Testing completed` and leaves its window open forever. Waiting on the process would wait forever too — and a run of a fixed number of cycles, which is the one case with no time limit to fall back on, would never be reported at all. So completion is read from the log, and `Testing stopped by user` is reported as stopped rather than passed, because a run somebody closed early is not a pass.

`Minutes=` is deliberately unused: it works, but loosely — `Minutes=2` stopped testing at 3:35 — and a tool that stops early would report a pass for a test that did not run as long as it was asked to. The runner's wall clock is exact, so it is the only clock in play.

## Notes

- Settings, run artifacts and logs live in `%LOCALAPPDATA%\RochStressTest\`. The unpacked tool folders are left byte-identical to what was downloaded.
- Closing the window stops whatever is running. There is never a stress test left going with no window to stop it.
- The bundled tools are third-party and carry their own licences — see the readme in each folder. Roch StressTest itself is under `LICENSE`.

## Health and safety

These tests exist to break things. Linpack in particular pulls more current than anything else here, and a 30 GB Linpack run on a marginal memory setting will fail a machine that has been "stable" for months. Watch temperatures, know the voltage limits for your memory before you start, and do not leave a first run unattended.
