use std::env;

fn main() {
    // The release version is sourced from the Git tag (passed via RYADM_VERSION
    // by `task release`), with a leading `v` stripped so it matches SemVer. In
    // a plain `cargo build` RYADM_VERSION is unset and we fall back to the
    // Cargo.toml version.
    let version = env::var("RYADM_VERSION")
        .ok()
        .map(|v| v.trim_start_matches('v').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").unwrap());

    println!("cargo:rustc-env=RYADM_VERSION={version}");
    // Rebuild when the tag-derived version changes.
    println!("cargo:rerun-if-env-changed=RYADM_VERSION");
}
