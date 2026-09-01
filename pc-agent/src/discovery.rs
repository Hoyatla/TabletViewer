//! mDNS service advertisement (zero-conf DNS).
//!
//! The agent registers itself as `_pcagent._tcp.local.` on the LAN so the
//! Android tablet can auto-discover it without typing the IP.
//!
//! The matching client side uses Android's `NsdManager` (built-in), which
//! speaks the same mDNS protocol out of the box.
//!
//! Reference: https://developer.android.com/training/connect-devices-wirelessly/nsd

use std::time::Duration;

use anyhow::{Context, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

const SERVICE_TYPE: &str = "_pcagent._tcp.local.";
const DEFAULT_INSTANCE: &str = "pcagent";

#[derive(Clone)]
pub struct MdnsHandle {
    /// Full name of the registered service (e.g. "pcagent._pcagent._tcp.local.").
    pub fullname: String,
    /// Keep the daemon alive for the duration of the program.
    _daemon: std::sync::Arc<ServiceDaemon>,
}

/// Register the agent on the LAN. Returns a handle whose `_daemon` must
/// be kept alive for as long as the service should be discoverable.
pub fn register(port: u16, instance: Option<&str>) -> Result<MdnsHandle> {
    let daemon = ServiceDaemon::new().context("create mDNS daemon")?;
    let instance = instance.unwrap_or(DEFAULT_INSTANCE);

    // TXT record: version + platform + API prefix. The tablet can read
    // them to decide compatibility and to display info.
    let version = env!("CARGO_PKG_VERSION");
    let properties: &[(&str, &str)] = &[
        ("version", version),
        ("platform", std::env::consts::OS),
        ("api", "v1"),
    ];

    // `enable_addr_auto()` means mdns-sd will discover the local IPs
    // and bind to them. Pass empty `addrs` for that.
    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        instance,
        &format!("{instance}.local."),
        "",  // addresses (let mdns-sd auto-discover)
        port,
        properties,
    )
    .context("build service info")?
    .enable_addr_auto();

    daemon
        .register(service_info)
        .context("register mDNS service")?;

    let fullname = format!("{instance}.{SERVICE_TYPE}");
    tracing::info!(
        "mDNS: advertising {} on port {} (type: {}, version={})",
        fullname,
        port,
        SERVICE_TYPE,
        version
    );

    Ok(MdnsHandle {
        fullname,
        _daemon: std::sync::Arc::new(daemon),
    })
}

/// One-shot synchronous discovery: find all `_pcagent._tcp.local.` services
/// on the LAN within a timeout. Returns the fullname + IP + port of each.
pub fn discover(timeout: Duration) -> Result<Vec<(String, std::net::IpAddr, u16)>> {
    let daemon = ServiceDaemon::new()?;
    let receiver = daemon.browse(SERVICE_TYPE)?;
    let mut found = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(event) = receiver.recv_timeout(Duration::from_millis(200)) {
            if let ServiceEvent::ServiceResolved(info) = event {
                let name = info.get_fullname().to_string();
                let port = info.get_port();
                let addrs: Vec<std::net::IpAddr> =
                    info.get_addresses().iter().copied().collect();
                for a in addrs {
                    if !found.iter().any(|(n, x, p)| n == &name && x == &a && *p == port) {
                        found.push((name.clone(), a, port));
                    }
                }
            }
        }
    }
    let _ = daemon.shutdown();
    Ok(found)
}
