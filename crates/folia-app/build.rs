// Embeds the original Folia application icon in Windows executables.

use std::io::{self, Write};

fn main() -> io::Result<()> {
    writeln!(io::stdout(), "cargo::rerun-if-changed=../../assets/icon.ico")?;
    if std::env::var_os("CARGO_CFG_TARGET_OS").as_deref() == Some(std::ffi::OsStr::new("windows")) {
        winresource::WindowsResource::new().set_icon("../../assets/icon.ico").compile()?;
    }
    Ok(())
}
