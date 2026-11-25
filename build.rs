//! A build with `src/config.rs` (a server and a token of one's own, kept out of git: see
//! `.gitignore`) signs itself in with it; without it (a checkout, CI) the app starts at its login
//! page, with `src/config_none.rs`. Looked for in the crate's own folder: `nsh package` builds
//! from another directory.
fn main() {
    let config = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("src/config.rs");
    println!("cargo::rustc-check-cfg=cfg(remux_local_config)");
    println!("cargo::rerun-if-changed={}", config.display());
    if config.exists() {
        println!("cargo::rustc-cfg=remux_local_config");
    }
}
