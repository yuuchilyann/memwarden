//! Embeds the icon and version information into memwarden.exe, so Explorer,
//! shortcuts and Task Manager show them.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/memwarden.ico");
    let icon = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets").join("memwarden.ico");
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let numbers = env::var("CARGO_PKG_VERSION_MAJOR").unwrap()
        + ","
        + &env::var("CARGO_PKG_VERSION_MINOR").unwrap()
        + ","
        + &env::var("CARGO_PKG_VERSION_PATCH").unwrap()
        + ",0";
    // rc.exe accepts forward slashes, which need no escaping in its strings.
    let icon = icon.display().to_string().replace('\\', "/");
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "memwarden"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "memwarden"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "memwarden.exe"
      VALUE "LegalCopyright", "MIT OR Apache-2.0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("memwarden.rc");
    std::fs::write(&path, rc).unwrap();
    embed_resource::compile(&path, embed_resource::NONE).manifest_optional().unwrap();
}
