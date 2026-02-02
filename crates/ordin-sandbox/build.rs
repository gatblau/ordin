//! Compiles in the SHA-256 digest of the Python guest (Sandbox.md *Internal Logic* A.4), so the
//! runtime refuses a guest whose bytes differ from the ones this build was made with.

use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let guest = manifest_dir.join("../../sandbox/sandbox-host/guest/pysandbox-guest.wasm");
    println!("cargo:rerun-if-changed={}", guest.display());
    let bytes = std::fs::read(&guest).expect("the Python guest is committed");
    let digest = Sha256::digest(&bytes);
    let mut hex = String::with_capacity(64);
    for b in digest {
        let _ = write!(hex, "{b:02x}");
    }
    println!("cargo:rustc-env=ORDIN_SANDBOX_PY_DIGEST={hex}");
}
