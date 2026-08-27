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
import main as m, toolset

root = ctk.CTk()
root.withdraw()
app = m.StressApp(root)
root.update_idletasks()

fails = 0
for tool in toolset.TOOLS:
    panel = app.panels.get(tool.key)
    if panel is None:
        print("SKIP", tool.name, "(not available)"); continue
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
                extra = " | cfg=" + os.path.basename(spec.argv[1])
            if tool.key == "linpack":
                extra = " | n=%s" % cfg.get("problem_size")
            if tool.key == "prime95":
                extra = " | mem=%sMB fft=%s-%s" % (cfg.get("memory"), cfg.get("min_fft"), cfg.get("max_fft"))
            print("OK  ", tool.name.ljust(13), preset.name.ljust(28), spec.summary[:52] + extra)
        except Exception as e:
            fails += 1
            print("FAIL", tool.name.ljust(13), preset.name.ljust(28), repr(e))
root.destroy()
print()
print("failures:", fails)
sys.exit(1 if fails else 0)
