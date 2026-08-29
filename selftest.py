"""Build every panel and every preset, and check each produces a real spec.

A launcher fails in a way unit tests miss: a preset that names a field the
tool does not have, or a discovered .cfg that has since been renamed, breaks
only when somebody opens that tab and presses Start. This walks every
combination the window can produce and builds the launch spec for each, so
that failure happens here instead.

Nothing is executed -- the specs are built and thrown away.

    py -V:3.13 selftest.py
"""
import os, sys
import customtkinter as ctk
import main as m
import tools as toolset

root = ctk.CTk()
root.withdraw()
app = m.StressApp(root)
root.update_idletasks()

fails = 0
for tool in toolset.TOOLS:
    panel = app.panels.get(tool.key)
    if panel is None and not tool.available(app.root_path):
        print("SKIP", tool.name, "(not available)"); continue
    if panel is None:
        # No tab, because the tool is configured in its own window. There is
        # no tab to check against, but the default still has to build -- that
        # is what the Quick Start button runs.
        try:
            spec = tool.build(tool.quick_config(app.root_path), app.root_path)
            print("OK  ", tool.name.ljust(13), "no tab:", spec.summary)
        except Exception as error:
            fails += 1
            print("FAIL", tool.name.ljust(13), "no tab, default:", repr(error))
        continue
    blocked = tool.unsupported_reason(app.root_path)
    if blocked:
        # A tool that cannot run here must refuse to build, not build
        # something that will exit having done nothing. That refusal is the
        # thing worth checking.
        try:
            tool.build(tool.quick_config(app.root_path), app.root_path)
        except Exception as error:
            print("OK   %-17s refuses to run here: %s" % (tool.name, str(error)[:52]))
        else:
            fails += 1
            print("FAIL %-17s is blocked but still built a launch spec" % tool.name)
        continue

    if not panel.presets():
        # A tool with nothing to preset would otherwise be skipped entirely
        # here, which is exactly the coverage worth keeping.
        cfg = panel.config()
        try:
            spec = tool.build(cfg, app.root_path)
            assert os.path.isfile(spec.argv[0]), "exe missing"
            print("OK   %-17s %-24s %s" % (tool.name, "(no presets)",
                                           spec.summary[:52]))
        except Exception as error:
            fails += 1
            print("FAIL %-17s %-24s %r" % (tool.name, "(no presets)", error))

    for preset in panel.presets():
        panel.preset_row.set(preset.name)
        panel.apply_preset(preset.name)
        root.update_idletasks()
        cfg = panel.config()
        try:
            spec = tool.build(cfg, app.root_path)
            exe_ok = os.path.isfile(spec.argv[0])
            cwd_ok = os.path.isdir(spec.cwd)
            assert exe_ok and cwd_ok, "exe %s cwd %s" % (exe_ok, cwd_ok)
            extra = ""
            if tool.key == "testmem5":
                extra = " | " + spec.cmdline.split("TM5.exe\" ")[-1]
            if tool.key == "linpack":
                extra = " | n=%s" % cfg.get("problem_size")
            if tool.key == "prime95":
                extra = " | mem=%sMB fft=%s-%s" % (cfg.get("memory"), cfg.get("min_fft"), cfg.get("max_fft"))
            print("OK  ", tool.name.ljust(13), preset.name.ljust(28), spec.summary[:52] + extra)
        except Exception as e:
            fails += 1
            print("FAIL", tool.name.ljust(13), preset.name.ljust(28), repr(e))
# The Quick Start page and each tool's own tab must agree on the default,
# because they are advertised as the same thing.
print()
for tool in toolset.TOOLS:
    panel = app.panels.get(tool.key)
    if panel is None:
        continue
    if tool.unsupported_reason(app.root_path):
        continue
    panel.apply_quick_start()
    root.update_idletasks()
    quick = tool.quick_config(app.root_path)
    tab = panel.config()
    # Values derived from free memory are recomputed each time they are
    # asked for -- that is the point of them -- so the two reads land a
    # fraction of a second and a megabyte or two apart. Compared exactly,
    # this test fails roughly one run in six for a difference that means
    # nothing. Everything else must match to the value.
    derived = {"memory", "problem_size", "leading_dimension"}         if hasattr(tool, "suggested_memory") or hasattr(tool, "apply_memory")         else set()

    def apart(key, left, right):
        if key not in derived:
            return left != right
        try:
            return abs(int(left) - int(right)) > max(8, int(left) * 0.01)
        except (TypeError, ValueError):
            return left != right

    # A field the preset locks is filled in by the tool itself -- Prime95
    # works its own FFT ranges out from the cache it finds -- so whatever is
    # left in the box never reaches the run, and the two sides having
    # different leftovers in it means nothing.
    locked = set(tool.locked_fields(tool.quick_preset_name(app.root_path)))

    differs = {k: (quick.get(k), tab.get(k)) for k in quick
               if k in tab and k not in locked and apart(k, quick[k], tab[k])}
    if differs:
        fails += 1
        print("FAIL", tool.name.ljust(13), "tab disagrees with Quick Start:", differs)
    else:
        print("OK  ", tool.name.ljust(13), "default:", tool.quick_summary(app.root_path))

root.destroy()
print()
print("failures:", fails)
sys.exit(1 if fails else 0)
