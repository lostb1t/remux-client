//! Finds the Chromecasts on the network (mDNS, `_googlecast._tcp`), and keeps the list current.

use std::net::IpAddr;
use mdns_sd::RecvTimeoutError;
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};

use mdns_sd::{ServiceDaemon, ServiceEvent};

use super::Device;

const SERVICE: &str = "_googlecast._tcp.local.";

/// Starts looking. The list is filled in by a thread of its own, for as long as the process runs.
pub fn start() -> Arc<Mutex<Vec<Device>>> {
    let found: Arc<Mutex<Vec<Device>>> = Arc::default();
    let list = found.clone();
    std::thread::Builder::new()
        .name("cast-discover".into())
        .spawn(move || {
            let daemon = match ServiceDaemon::new() {
                Ok(daemon) => daemon,
                Err(err) => {
                    log::warn!("cast: no discovery: {err}");
                    return;
                }
            };
            log::info!("cast: looking for Chromecasts");
            let mut events = match daemon.browse(SERVICE) {
                Ok(events) => events,
                Err(err) => {
                    log::warn!("cast: could not browse: {err}");
                    return;
                }
            };
            let mut last_search = Instant::now();
            loop {
                // Asking again now and then finds what was switched on after the first time.
                if last_search.elapsed() >= super::SEARCH_EVERY && super::searching() {
                    let _ = daemon.stop_browse(SERVICE);
                    match daemon.browse(SERVICE) {
                        Ok(again) => events = again,
                        Err(err) => log::warn!("cast: could not browse again: {err}"),
                    }
                    last_search = Instant::now();
                }
                let event = match events.recv_timeout(Duration::from_secs(1)) {
                    Ok(event) => event,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => {
                        // A search that was stopped ends its channel: the next one is made at once.
                        match daemon.browse(SERVICE) {
                            Ok(again) => {
                                events = again;
                                continue;
                            }
                            Err(_) => break,
                        }
                    }
                };
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        // The address a connection can use: IPv4, as a link-local IPv6 one
                        // needs a scope the socket doesn't have.
                        let host = info.get_addresses_v4().into_iter().next().map(IpAddr::V4);
                        let Some(host) = host else { continue };
                        let property = |key: &str| info.get_property_val_str(key).map(str::to_string);
                        let device = Device {
                            id: property("id").unwrap_or_else(|| info.get_fullname().to_string()),
                            name: property("fn").unwrap_or_else(|| info.get_fullname().to_string()),
                            model: property("md").unwrap_or_default(),
                            host,
                            port: info.get_port(),
                            service: info.get_fullname().to_string(),
                        };
                        log::info!("cast: found {} ({}) at {}", device.name, device.model, device.host);
                        let mut list = list.lock().unwrap();
                        list.retain(|d| d.id != device.id);
                        list.push(device);
                        list.sort_by(|a, b| a.name.cmp(&b.name));
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        list.lock().unwrap().retain(|d| d.service != fullname);
                    }
                    _ => {}
                }
            }
        })
        .ok();
    found
}
