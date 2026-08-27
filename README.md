# Roch StressTest

**Roch StressTest** — one window for every stress test on the machine. Prime95, y-cruncher, TestMem5, RAM Test Pro and Linpack, each with presets that say what they actually do, a time limit, live failure detection, and a queue that chains them into one unattended stability run. Python 3.13 / CustomTkinter, same theme as [Roch Viewer](https://github.com/RochStudio/Roch-Viewer) and [Roch GPU OC](https://github.com/RochStudio/Roch-GPU).

It does not implement a single test. Every tool runs exactly as it would if you started it yourself; what this adds is the part they leave to you.

```
Roch StressTest/
├─ main.py                 the window: Quick Start, tool tabs, queue, log
├─ runner.py               starts a test, watches it, enforces the time limit
├─ errors.py               the patterns that decide a run has failed
├─ toolset.py              the registry of tools, in tab order
├─ tool_*.py               one adapter per tool
├─ theme.py                the shared Roch palette
├─ widgets.py              the field/section widgets the panels are built from
├─ hardware.py             cores, RAM and CPU vendor, for the defaults
└─ <tool folders>          Prime95, y-cruncher, TestMem5, RAM Test Pro, Linpack
```

## Run it

```bat
RUN_AS_ADMIN.bat
```

Or build a standalone EXE with `BUILD_EXE.bat` and copy `dist\RochStressTest.exe` up one level, next to the tool folders — it looks for them beside itself.

Administrator rights are asked for once at launch. TestMem5 and RAM Test Pro need them to lock physical pages, Prime95 to set affinity. Asking once beats each tool failing differently three hours into a queue.

## What each tab does

**Quick Start** is the front page: one card per tool, each running that tool's default configuration with a single button, plus **Queue all** to chain every default in order. Each tool's own tab opens on the same defaults, so the two never disagree about what "default" means.

| Tool | Presets | Driven by |
|---|---|---|
| **Prime95** | Smallest / Small / Large / Blend / Huge FFTs | `prime.txt` in a private work directory, then `prime95 -W<dir> -t` |
| **y-cruncher** | All algorithms, Vector+FFT, VSTv3, in-cache, memory-heavy | `y-cruncher … stress -M -D -TL <algorithms>` |
| **TestMem5** | every `.cfg` in `bin/`, anta777 profiles first | `Config File="name.cfg"`, cycle count rewritten |
| **RAM Test Pro** | every `.cfg` in `config/` | `config/current_config.txt` |
| **Linpack** | 2 / 4 / 6 / 8 / 14 / 30 GB | the raw Intel MKL binary with a generated input file |

Memory figures are computed from what is actually free when you pick a preset, not from a constant. Linpack's problem size and leading dimension are derived from the memory box using Intel's own rule — the nearest odd multiple of 16 at or above the problem size on AVX parts.

## Defaults

What each tool runs from Quick Start, and what its own tab opens on:

| Tool | Default |
|---|---|
| Prime95 | Smallest FFTs (4K–32K), 60 min |
| y-cruncher | All algorithms, 60 min |
| TestMem5 | 1usmus v3 @ 1usmus, 25 cycles, no time limit |
| RAM Test Pro | DDR4_DDR5_universal, no time limit |
| Linpack | 4 GB, 30 min |

TM5's cycle count lives inside the `.cfg`, so overriding it means writing a copy. That copy goes to `bin/MT.cfg` — the name TM5 falls back to anyway — with only the `Cycles=` line changed and every other byte identical. The stock profiles are never edited: a cycle count set here is a property of the run, not of the profile.

## Failure detection

The point of the program. What each tab claims is what it does:

- **y-cruncher** and **Linpack** are read live from the process. y-cruncher prints `Running VSTv3: Passed` once per iteration and the negative of that line stops the run within a second.
- **Linpack** additionally has every result row parsed. Linpack does *not* stop on a bad solve — it writes something other than `pass` in the eighth column and carries on — so that column is checked, and so is the residual, which must not drift between identical trials.
- **Prime95** is watched through `results.txt`, which it writes on every error. A worker that fails while you are away is still caught.
- **TestMem5** is read from `Log.txt`, which 0.13.1 appends beside `TM5.exe`. `Error in test #N`, a non-zero error total, and a crash blamed on the tested memory all stop the run. TM5's *own* failures — memory it could not allocate or lock — are deliberately not reported as instability, because TM5 says outright that they are not.
- **RAM Test Pro** reports only in its own window and writes no log. A crash or early exit is caught; the on-screen counter cannot be. Leave that window visible.

A failure kills the process tree, stops the queue, beeps, switches to the Log tab, and writes a transcript to `%LOCALAPPDATA%\RochStressTest\logs\FAILED-<timestamp>.txt` — because the run that matters is the one that failed at 4am.

## The queue

Set a test up on its tab, press **Add to queue**, repeat, then **Run queue**. Steps run top to bottom and the first failure ends the run. A sensible overnight order is TestMem5 → y-cruncher → Prime95 → Linpack: memory first, because a memory fault will fail the CPU tests too and send you looking in the wrong place.

Give every step a time limit. A step set to `0` never finishes on its own and the queue will stop there.

## Four things found while building this

Both are handled in the code; they are written down because they cost an afternoon each and neither is documented anywhere obvious.

**Prime95 torture settings do not live in `local.txt`.** Every reference, including Prime95's own `undoc.txt`, says they do. In 30.19 that is a migration path: give it a `local.txt` and it folds those keys into `prime.txt`, deletes `local.txt`, and carries on. `tool_prime95.py` writes `prime.txt` directly. Verifiable — ask for `MinTortureFFT=MaxTortureFFT=1024` and `results.txt` says `Self-test 1024K passed!` and nothing else.

**Linpack needs `MKL_DEBUG_CPU_TYPE=5` on Zen 5.** Without it `linpack_amd64.exe` dies with an illegal instruction before printing a single result row — its kernel dispatch picks a path the CPU does not implement. The documented `MKL_ENABLE_INSTRUCTIONS=AVX2` does not fix it; only this does. The Intel-branded builds (`linpack_intel64.exe`, `linpack_xeon64.exe`) refuse to run on AMD at all, exiting cleanly with a message rather than an error, which otherwise looks like a test that finished instantly — so that case is caught and reported.

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
