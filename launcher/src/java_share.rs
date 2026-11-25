//! Sharing a world the real game opened to LAN, and joining a friend's.
//!
//! Minecraft's "Open to LAN" listens on a port it picks, reachable on this network only. This
//! finds that port (the game writes it to `logs/latest.log`), asks the router to forward a fixed
//! TCP port to it (`upnp`), and gives the player one address to send: `102.186.255.46:25565`
//! instead of a router's settings page. The mapping goes away when sharing stops.

use crate::upnp;
use std::io::{Read, Seek, SeekFrom};
use std::net::{Ipv4Addr, UdpSocket};
use std::path::Path;

/// The port on the router side: vanilla's, so friends who know the game don't need a number.
pub const PORT: u16 = 25565;

/// What the host sends to friends.
#[derive(Clone, Debug, Default)]
pub struct Share {
    /// `192.168.1.2:54321`: this network only, always right.
    pub lan: String,
    /// `102.186.255.46:25565`: the internet, when the router took the mapping.
    pub internet: Option<String>,
    /// Why there is no internet address, or what to tell friends.
    pub note: Option<String>,
    /// What to take away when sharing stops.
    pub(crate) mapping: Option<(upnp::Gateway, u16)>,
}

impl Share {
    /// The mapping is dropped when the launcher closes too: nothing forwards a port with no
    /// world behind it.
    pub fn forget_mapping(&mut self) {
        if let Some((gateway, port)) = self.mapping.take() {
            // Home gateways answer "Invalid Args" while they are busy: ask again.
            for _ in 0..3 {
                if upnp::unmap(&gateway, port).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(700));
            }
        }
    }
}

/// The LAN port the game opened, if it opened one, from `offset` bytes into the log: the new
/// offset, so the next read only looks at what the game appended since.
pub fn port_after(log: &Path, offset: u64) -> (u64, Option<u16>) {
    let Ok(mut file) = std::fs::File::open(log) else {
        return (0, None);
    };
    let length = file.metadata().map(|m| m.len()).unwrap_or(0);
    // A new log (the game restarted) starts over.
    let start = if length < offset { 0 } else { offset };
    if file.seek(SeekFrom::Start(start)).is_err() && file.seek(SeekFrom::Start(0)).is_err() {
        return (0, None);
    }
    let mut text = String::new();
    if file.read_to_string(&mut text).is_err() {
        return (length, None);
    }
    (length, port_in(&text))
}

/// The port of the last world opened to LAN in a piece of the game's log, if it is still open.
fn port_in(text: &str) -> Option<u16> {
    let mut port = None;
    for line in text.lines() {
        if line.contains("Stopping server") {
            port = None;
        }
        for marker in ["Started serving on ", "Local game hosted on port "] {
            if let Some(rest) = line.split(marker).nth(1) {
                port = rest
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|digits| digits.parse().ok());
            }
        }
    }
    port
}

/// This computer's address on the network the game listens on.
pub fn local_ipv4() -> Option<Ipv4Addr> {
    // No packet is sent: connecting a UDP socket only picks the route's own address.
    let socket = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect(("8.8.8.8", 80)).ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// The address friends on this network paste: the LAN one, else the port alone.
pub fn lan_address(port: u16) -> String {
    match local_ipv4() {
        Some(ip) => format!("{ip}:{port}"),
        None => format!("localhost:{port}"),
    }
}

/// Forward a port to `port`, then name the address friends on the internet paste.
pub fn share(port: u16) -> Share {
    let lan = lan_address(port);
    let Some(ip) = local_ipv4() else {
        return Share {
            lan,
            note: Some(
                "This computer is not on a network the game can be reached from.".to_string(),
            ),
            ..Share::default()
        };
    };
    let gateway = match upnp::discover() {
        Ok(gateway) => gateway,
        Err(e) => {
            return Share {
                lan,
                note: Some(forward_hint(&ip, &e)),
                ..Share::default()
            };
        }
    };
    let Some(external_port) = forward(&gateway, &ip, port) else {
        return Share {
            lan,
            note: Some(forward_hint(&ip, "the router did not forward the port")),
            ..Share::default()
        };
    };
    let external = match upnp::external_ip(&gateway) {
        Ok(ip) => ip,
        // Some gateways forward happily but cannot say the address (Huawei's answer "Action
        // Failed"): a public service names it instead.
        Err(_) => match public_ip() {
            Some(ip) => ip,
            None => {
                let _ = upnp::unmap(&gateway, external_port);
                return Share {
                    lan,
                    note: Some(
                        "The router forwarded the port but neither it nor the internet could say \
                         this connection's public address."
                            .to_string(),
                    ),
                    ..Share::default()
                };
            }
        },
    };
    if let Some(note) = carrier_nat(&external) {
        let _ = upnp::unmap(&gateway, external_port);
        return Share {
            lan,
            note: Some(note),
            ..Share::default()
        };
    }
    Share {
        lan,
        internet: Some(format!("{external}:{external_port}")),
        note: None,
        mapping: Some((gateway, external_port)),
    }
}

/// Maps `port` to this computer, on the first free port from [`PORT`] up.
fn forward(gateway: &upnp::Gateway, ip: &Ipv4Addr, port: u16) -> Option<u16> {
    for external in PORT..=PORT + 5 {
        match upnp::map(gateway, external, &ip.to_string(), port, 3600) {
            Ok(()) => return Some(external),
            // 718: a mapping already takes this port. Ours from an earlier session is replaced;
            // anyone else's is left alone and the next port is tried.
            Err(e) if e.ends_with("(718)") => {
                let ours = upnp::mapping(gateway, external)
                    .is_some_and(|description| description.starts_with("Horizon"));
                if ours {
                    let _ = upnp::unmap(gateway, external);
                    if upnp::map(gateway, external, &ip.to_string(), port, 3600).is_ok() {
                        return Some(external);
                    }
                }
            }
            Err(_) => {}
        }
    }
    None
}

/// `192.168.` and friends on the router's outside mean the provider put the home behind another
/// router (carrier-grade NAT): forwarding cannot work from here.
fn carrier_nat(external: &str) -> Option<String> {
    let ip: Ipv4Addr = external.parse().ok()?;
    let octets = ip.octets();
    let shared = octets[0] == 100 && (64..128).contains(&octets[1]);
    (ip.is_private() || shared).then(|| {
        format!(
            "Your provider puts this connection behind another network ({external}), so opening \
             a port here is not enough: use a tunnel or a VPN such as Radmin VPN or Tailscale."
        )
    })
}

/// This computer's address as the internet sees it, asked of a small public service: the router
/// is the authority, but it is exactly what did not answer.
pub fn public_ip() -> Option<String> {
    crate::backend::get("https://api.ipify.org")
        .ok()?
        .into_string()
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| text.parse::<Ipv4Addr>().is_ok())
}

/// What to do when the router did not forward the port.
fn forward_hint(ip: &Ipv4Addr, why: &str) -> String {
    let forwarded = match public_ip() {
        Some(public) => format!(
            "or forward TCP {PORT} to {ip} in the router, and friends use {public}:{PORT}"
        ),
        None => format!("or forward TCP {PORT} to {ip} in the router"),
    };
    format!(
        "Friends on this Wi-Fi can join at this computer. For friends on the internet, the router \
         has to forward a port ({why}). Turn on UPnP in the router's settings (usually Advanced → \
         UPnP) and press Try again, {forwarded}."
    )
}

/// A world's address as a friend types it: `host`, `host:port`, or nothing.
pub fn address(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return None;
    }
    let (host, port) = match text.rsplit_once(':') {
        Some((host, port)) => {
            let port: u16 = port.parse().ok()?;
            if port == 0 {
                return None;
            }
            (host, Some(port))
        }
        None => (text, None),
    };
    let host = host.trim_matches(['[', ']']);
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') {
        return None;
    }
    let looks_like_host = host.parse::<Ipv4Addr>().is_ok()
        || host.contains('.')
        || host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::Ipv6Addr>().is_ok();
    if !looks_like_host {
        return None;
    }
    Some(match port {
        Some(port) => format!("{host}:{port}"),
        None => format!("{host}:{PORT}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_the_port_the_game_opened() {
        let log = "[00:00:01] [Render thread/INFO]: Stopping worker threads\n\
                   [00:00:02] [Render thread/INFO]: Started serving on 54321\n\
                   [00:00:03] [Render thread/INFO]: Started http server on port\n";
        assert_eq!(port_in(log), Some(54321));
        // An older wording, and the world closing clears it.
        assert_eq!(port_in("Local game hosted on port 51234\n"), Some(51234));
        assert_eq!(
            port_in("Started serving on 54321\n[00:01:00] [Server thread/INFO]: Stopping server\n"),
            None
        );
    }

    #[test]
    fn reads_only_what_the_log_appended() {
        let dir = std::env::temp_dir().join(format!("horizon-share-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("latest.log");
        std::fs::write(&log, "Started serving on 50000\n").unwrap();
        let (offset, port) = port_after(&log, 0);
        assert_eq!(port, Some(50000));
        assert_eq!(offset, "Started serving on 50000\n".len() as u64);
        let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(file, "Stopping server").unwrap();
        let (_, port) = port_after(&log, offset);
        assert_eq!(port, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn friends_type_ip_or_ip_port() {
        assert_eq!(address("102.186.255.46"), Some("102.186.255.46:25565".into()));
        assert_eq!(address("102.186.255.46:25566"), Some("102.186.255.46:25566".into()));
        assert_eq!(address(" 192.168.1.2:54321 "), Some("192.168.1.2:54321".into()));
        assert_eq!(address("play.example.com"), Some("play.example.com:25565".into()));
        assert_eq!(address("localhost"), Some("localhost:25565".into()));
        assert_eq!(address(""), None);
        assert_eq!(address("not an address"), None);
        assert_eq!(address("2A7KQ-M3XPD-8Z3QF"), None);
        assert_eq!(address("1.2.3.4:0"), None);
        assert_eq!(address("1.2.3.4:99999"), None);
    }

    #[test]
    fn a_private_router_address_is_carrier_nat() {
        assert!(carrier_nat("192.168.1.1").is_some());
        assert!(carrier_nat("100.72.1.1").is_some());
        assert!(carrier_nat("102.186.255.46").is_none());
    }
}
