//! Finding a paired iPhone on the local network.
//!
//! When "sync over Wi-Fi" is enabled, iOS advertises itself through mDNS as
//! `_apple-mobdev2._tcp` with its UDID in the TXT record. Resolving that lets
//! AutoSign reach the phone directly, which is what makes renewals work after
//! the cable is unplugged - Apple's Windows service does not always list
//! network devices itself.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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
    let mut seen: Vec<String> = Vec::new();

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(event) = receiver.recv_timeout(remaining.min(Duration::from_millis(400))) else {
            continue;
        };

        let mdns_sd::ServiceEvent::ServiceResolved(service) = event else {
            continue;
        };

        seen.push(format!(
            "{} {:?}",
            service.get_fullname(),
            service.get_addresses_v4()
        ));

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

    if result.is_none() {
        if seen.is_empty() {
            log::info!(
                "mDNS: no {APPLE_WIFI_SERVICE} service was advertised on this network"
            );
        } else {
            log::info!("mDNS: saw {} Apple device advert(s): {seen:?}", seen.len());
        }
    }

    result
}

/// Looks up `mac` (the phone's Wi-Fi address) in the operating system's ARP
/// table.
///
/// This is a fallback for networks where mDNS is filtered: the address is
/// already known from the USB session, so the ARP table can tell us the current
/// IP without any multicast traffic.
pub fn arp_address_for_mac(mac: &str) -> Option<IpAddr> {
    let wanted = normalize_mac(mac);
    if wanted.is_empty() {
        return None;
    }

    // `arp -a` prints the neighbours of every interface; only the MAC and the
    // address have to be extracted, which is language independent.
    let output = std::process::Command::new("arp").arg("-a").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);

    for line in text.lines() {
        if !line.contains(&*mac) && !line.contains(&wanted) {
            // Fall through to the normalised comparison below anyway.
        }

        let mut address = None;
        for token in line.split_whitespace() {
            if let Ok(candidate) = token.parse::<IpAddr>() {
                address = Some(candidate);
            }
        }

        if let Some(address) = address {
            if normalize_mac(line).contains(&wanted) {
                return Some(address);
            }
        }
    }

    None
}

/// Scans the local /24 for hosts that accept connections on `port`.
///
/// Used as a last resort when neither mDNS nor ARP can find the phone: iOS
/// answers on 62078 when wireless sync is enabled, and the caller verifies the
/// identity of whichever address answers.
pub async fn scan_local_subnet(port: u16, per_host_timeout: Duration) -> Vec<IpAddr> {
    let Some(local) = local_ipv4() else {
        return Vec::new();
    };

    let octets = local.octets();
    let mut tasks = Vec::new();

    for host in 1u8..=254 {
        let candidate = Ipv4Addr::new(octets[0], octets[1], octets[2], host);
        if candidate == local {
            continue;
        }

        tasks.push(tokio::spawn(async move {
            let address = SocketAddr::from((candidate, port));
            match tokio::time::timeout(per_host_timeout, tokio::net::TcpStream::connect(address))
                .await
            {
                Ok(Ok(_)) => Some(IpAddr::V4(candidate)),
                _ => None,
            }
        }));
    }

    let mut found = Vec::new();
    for task in tasks {
        if let Ok(Some(address)) = task.await {
            found.push(address);
        }
    }

    found
}

/// The address this computer uses for outgoing traffic.
fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;

    match socket.local_addr().ok()? {
        SocketAddr::V4(address) => Some(*address.ip()),
        SocketAddr::V6(_) => None,
    }
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

/// MAC addresses show up with different separators depending on the source.
fn normalize_mac(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
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

    #[test]
    fn mac_normalisation_ignores_separators() {
        assert_eq!(normalize_mac("A1:B2:C3:D4:E5:F6"), "a1b2c3d4e5f6");
        assert_eq!(normalize_mac("a1-b2-c3-d4-e5-f6"), "a1b2c3d4e5f6");
    }
}
