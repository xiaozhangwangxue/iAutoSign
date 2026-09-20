//! Finding a paired iPhone on the local network.
//!
//! When "sync over Wi-Fi" is enabled, iOS advertises itself through mDNS as
//! `_apple-mobdev2._tcp` with its UDID in the TXT record. Resolving that lets
//! AutoSign reach the phone directly, which is what makes renewals work after
//! the cable is unplugged - Apple's Windows service does not always list
//! network devices itself.

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

/// The service type Apple's wireless sync advertises.
pub const APPLE_WIFI_SERVICE: &str = "_apple-mobdev2._tcp.local.";

/// Browses for `udid` and returns the address it can be reached at.
pub fn discover_address(udid: &str, timeout: Duration) -> Option<IpAddr> {
    let daemon = mdns_sd::ServiceDaemon::new().ok()?;
    let receiver = daemon.browse(APPLE_WIFI_SERVICE).ok()?;

    let wanted = normalize_udid(udid);
    let deadline = Instant::now() + timeout;
    let mut result = None;

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(event) = receiver.recv_timeout(remaining.min(Duration::from_millis(400))) else {
            continue;
        };

        let mdns_sd::ServiceEvent::ServiceResolved(service) = event else {
            continue;
        };

        // The TXT record carries the UDID (`deviceid`), which is the reliable
        // way to tell phones apart; fall back to the instance name.
        let matches_device = service
            .get_property_val_str("deviceid")
            .map(|id| normalize_udid(id) == wanted)
            .unwrap_or_else(|| normalize_udid(service.get_fullname()).contains(&wanted));

        if !matches_device {
            continue;
        }

        let address = service
            .get_addresses_v4()
            .into_iter()
            .next()
            .map(IpAddr::V4);

        if let Some(address) = address {
            result = Some(address);
            break;
        }
    }

    let _ = daemon.shutdown();
    result
}

/// Convenience wrapper for callers that only care about IPv4 addresses.
pub fn discover_ipv4(udid: &str, timeout: Duration) -> Option<Ipv4Addr> {
    match discover_address(udid, timeout) {
        Some(IpAddr::V4(address)) => Some(address),
        _ => None,
    }
}

/// UDIDs appear with and without separators depending on where they come from.
fn normalize_udid(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udid_normalisation_ignores_separators_and_case() {
        assert_eq!(
            normalize_udid("00008110-001A2C3D4E5F001E"),
            normalize_udid("00008110001a2c3d4e5f001e")
        );
        assert_eq!(normalize_udid("AB:CD-EF"), "abcdef");
    }
}
