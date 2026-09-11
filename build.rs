//! Build script: capture the exact compiler identity so every evidence receipt
//! can bind it. This is a few milliseconds at build time and removes any doubt
//! about which toolchain produced a measured artefact.

use std::process::Command;

fn main() {
    let out = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string()))
        .arg("-V")
        .output();
    let version = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    };
    println!("cargo:rustc-env=RUSTC_VERSION={version}");
    println!("cargo:rerun-if-changed=build.rs");
}
