//! Stamp the executable with its icon, its version and its manifest.
//!
//! The version numbers come from Cargo.toml, the one place the version is
//! written, so the window title and the file properties can never disagree --
//! the PyInstaller build once shipped a hand-typed 1.0.0.0 in its resource
//! while the title moved on.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=icon.ico");
    println!("cargo:rerun-if-changed=res/RochStressTest.manifest");
    println!("cargo:rerun-if-changed=Cargo.toml");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut numbers: Vec<u16> = version
        .split(['.', '-', '+'])
        .take(3)
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    numbers.resize(4, 0);
    let commas = numbers
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let icon = manifest_dir.join("icon.ico");
    let manifest = manifest_dir.join("res").join("RochStressTest.manifest");
    let escape = |path: &PathBuf| path.display().to_string().replace('\\', "\\\\");

    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 24 "{manifest}"

1 VERSIONINFO
FILEVERSION {commas}
PRODUCTVERSION {commas}
FILEFLAGSMASK 0x3f
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Roch Studio"
      VALUE "FileDescription", "Roch StressTest"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "RochStressTest"
      VALUE "LegalCopyright", "GPL-3.0"
      VALUE "OriginalFilename", "RochStressTest.exe"
      VALUE "ProductName", "Roch StressTest"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 1033, 1200
  END
END
"#,
        icon = escape(&icon),
        manifest = escape(&manifest),
    );

    let out = PathBuf::from(env::var("OUT_DIR").expect("out dir")).join("RochStressTest.rc");
    fs::write(&out, rc).expect("write resource script");
    embed_resource::compile_for(&out, ["RochStressTest"], embed_resource::NONE)
        .manifest_required()
        .expect("compile the version resource");
}
