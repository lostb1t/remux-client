//! What the app tells the server about the device it runs on (Remux's `APP_HOST`): made once.

use std::sync::OnceLock;

/// The device and the app, as the server is told.
#[derive(Debug, Clone)]
pub struct AppHost {
    /// The id of this device, kept as Jellyfin keeps it.
    pub device_id: String,
    /// The kind of device: the server shows it in its list of devices.
    pub device_name: String,
    pub remux_version: String,
}

static HOST: OnceLock<AppHost> = OnceLock::new();

/// The device and the app. Made on first use, on the engine's thread (it reads the storage).
pub fn app_host() -> &'static AppHost {
    HOST.get_or_init(|| AppHost {
        device_id: crate::session::device_id(),
        device_name: device_name(),
        remux_version: option_env!("REMUX_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")).to_owned(),
    })
}

/// What the server lists the device as: on Android its model ("XGIMI X50 Ultra Max", as
/// Remux's `whoami::devicename`), elsewhere the platform's name.
fn device_name() -> String {
    #[cfg(target_os = "android")]
    {
        let prop = |name: &str| {
            std::process::Command::new("getprop")
                .arg(name)
                .output()
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
                .unwrap_or_default()
        };
        let model = prop("ro.product.model");
        if !model.is_empty() {
            return model;
        }
    }
    nsh_core::prelude::platform_name().to_owned()
}
