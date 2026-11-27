//! A build with `src/config.rs` (a server and a token of one's own, kept out of git: see
//! `.gitignore`) signs itself in with it; without it (a checkout, CI) the app starts at its login
//! page, with `src/config_none.rs`. Looked for in the crate's own folder: `nsh package` builds
//! from another directory.
//!
//! On a Mac it tells the video player where `scripts/build-macos-framework.sh` built its framework
//! (`macos/build/framework`), for a run without an app around it (`nsh run`): the player loads it
//! as it starts (`player/mpv_mac.rs`), as NSH builds the app as a program of its own, which link
//! arguments of this crate do not reach.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
        println!("cargo::rustc-env=REMUX_MPV_BUILD_DIR={}", manifest.join("macos/build/framework").display());
    }
    let config = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("src/config.rs");
    println!("cargo::rustc-check-cfg=cfg(remux_local_config)");
    println!("cargo::rerun-if-changed={}", config.display());
    if config.exists() {
        println!("cargo::rustc-cfg=remux_local_config");
    }
}
