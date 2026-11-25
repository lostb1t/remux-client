//! A build with `src/config.rs` (a server and a token of one's own, kept out of git: see
//! `.gitignore`) signs itself in with it; without it (a checkout, CI) the app starts at its login
//! page, with `src/config_none.rs`.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(remux_local_config)");
    println!("cargo::rerun-if-changed=src/config.rs");
    if std::path::Path::new("src/config.rs").exists() {
        println!("cargo::rustc-cfg=remux_local_config");
    }
}
