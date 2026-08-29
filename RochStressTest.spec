# -*- mode: python ; coding: utf-8 -*-
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
