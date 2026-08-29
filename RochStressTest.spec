# -*- mode: python ; coding: utf-8 -*-
import os
import re as _re
# PyInstaller spec for Roch StressTest.
# Build with: py -m PyInstaller -y RochStressTest.spec

from PyInstaller.utils.hooks import collect_data_files

# customtkinter needs its theme/data files collected.
datas = collect_data_files('customtkinter')

datas += [
    ('icon.ico', '.'),
]

# The tool adapters are reached through the registry in tools/__init__.py, which
# imports each one by name, so the dependency scan does find them. They are
# listed anyway to say what the build is expected to carry -- and because a
# missing adapter shows up as a silently absent tab rather than as an error.
hiddenimports = [
    'core.version',
    'customtkinter',
    'app.theme',
    'app.widgets',
    'core.settings',
    'core.hardware',
    'core.errors',
    'core.runner',
    'core.toolbase',
    'core.memory',
    'core.winui',
    'tools',
    'tools.prime95',
    'tools.ycruncher',
    'tools.testmem5',
    'tools.ramtest',
    'tools.linpack',
    'tools.occt',
    'tools.cinebench',
    'tools.memtest_vulkan',
    'tools.threedmark11',
]

a = Analysis(
    ['main.py'],
    pathex=[],
    binaries=[],
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
    optimize=0,
)
# file_version_info.txt says it is generated from version.py. It was not:
# it held a hand-typed (1, 0, 0, 0), so bumping __version__ moved the window
# title and left the binary claiming the old number. Written here, from the
# same VERSION_TUPLE the docstring in version.py promises is used.
import io as _io
import runpy as _runpy

_version = _runpy.run_path(os.path.join("core", "version.py"))
_numbers = ", ".join(str(part) for part in _version["VERSION_TUPLE"])
_resource = _io.open("file_version_info.txt", encoding="utf-8").read()
_resource = _re.sub(r"(file|prod)vers=\([^)]*\)",
                    lambda m: m.group(1) + "vers=(" + _numbers + ")", _resource)
_resource = _re.sub(r"(StringStruct\(u?'(?:File|Product)Version', u?')[^']*'",
                    lambda m: m.group(1) + _version["__version__"] + "'",
                    _resource)
_io.open("file_version_info.txt", "w", encoding="utf-8",
         newline="\n").write(_resource)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='RochStressTest',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    icon='icon.ico',
    # TestMem5 and RAM Test Pro need administrator rights to lock physical
    # pages, and Prime95 needs them to set affinity. Asking once at launch
    # beats each tool failing differently three hours into a queue.
    uac_admin=True,
    version='file_version_info.txt',
)
