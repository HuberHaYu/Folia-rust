// Generates the Traditional Chinese locale from zh-CN so it is never edited by hand.

use std::path::PathBuf;
use std::{env, fs, io};

use zhconv::{Variant, zhconv};

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed=locales/zh-CN.toml");

    let out_dir = env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("OUT_DIR is not set"))?;
    let simplified = fs::read_to_string("locales/zh-CN.toml")?;
    fs::write(PathBuf::from(out_dir).join("zh-TW.toml"), zhconv(&simplified, Variant::ZhTW))
}
