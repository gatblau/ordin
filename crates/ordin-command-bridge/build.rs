//! The test programs of this crate link Tauri, which on Windows imports `TaskDialogIndirect` from
//! `comctl32.dll`. That function exists only in Common Controls version 6, which a program gets
//! through its manifest; without one, a test program fails to start (`STATUS_ENTRYPOINT_NOT_FOUND`).
//! The desktop program carries its own manifest; this one is for the tests only.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // The target, not the host this script runs on.
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && env == "msvc" {
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
