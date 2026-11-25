//! Universal Plug and Play: the router's own port forwarding.
//!
//! A world opened to LAN listens on a port only this network can reach. The router is the one
//! that can forward the internet's connections to it, and most routers offer UPnP for that:
//! this module finds the router (SSDP), asks it to map a TCP port to this computer
//! (`AddPortMapping`), asks which address the world wide web sees (`GetExternalIPAddress`), and
//! takes the mapping away when sharing stops.
//!
//! No crate: the two protocols are small. SSDP is one UDP datagram; UPnP is SOAP over HTTP,
//! which `ureq` sends.

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

/// Where a router's UPnP control lives.
#[derive(Clone, Debug)]
pub struct Gateway {
    /// The description document's URL (relative `controlURL`s resolve against it).
    pub location: String,
    /// The `WANIPConnection` (or `WANPPPConnection`) control URL.
    pub control: String,
    /// That service's type, which every SOAP action names.
    pub service: String,
}

/// The router that answers UPnP, if one does.
pub fn discover() -> Result<Gateway, String> {
    let locations = search()?;
    let mut problem = String::new();
    for location in locations {
        match from_description(&location) {
            Ok(gateway) => return Ok(gateway),
            Err(e) => problem = format!("{location}: {e}"),
        }
    }
    Err(match problem.is_empty() {
        true => "No router answered UPnP.".to_string(),
        false => problem,
    })
}

/// The `LOCATION` of every UPnP device that answers the search, the gateway devices first.
fn search() -> Result<Vec<String>, String> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| format!("UPnP socket: {e}"))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(700)))
        .map_err(|e| format!("UPnP socket: {e}"))?;
    let target: SocketAddr = "239.255.255.250:1900".parse().expect("the SSDP address is fixed");
    let mut locations: Vec<String> = Vec::new();
    for search in [
        "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
        "urn:schemas-upnp-org:device:InternetGatewayDevice:2",
        "upnp:rootdevice",
    ] {
        let message = format!(
            "M-SEARCH * HTTP/1.1\r\n\
             HOST: 239.255.255.250:1900\r\n\
             MAN: \"ssdp:discover\"\r\n\
             MX: 2\r\n\
             ST: {search}\r\n\r\n"
        );
        let _ = socket.send_to(message.as_bytes(), target);
    }
    // Answers come from more than one device, and some routers answer twice: collect until the
    // first is silent for a while.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut buffer = [0u8; 4096];
    while Instant::now() < deadline {
        match socket.recv_from(&mut buffer) {
            Ok((read, _)) => {
                let text = String::from_utf8_lossy(&buffer[..read]);
                if let Some(location) = header(&text, "location") {
                    if !locations.contains(&location) {
                        locations.push(location);
                    }
                }
            }
            Err(_) => break,
        }
    }
    if locations.is_empty() {
        return Err("No router answered UPnP.".to_string());
    }
    Ok(locations)
}

/// A header's value in an SSDP answer, whatever its case (`LOCATION`, `Location`).
fn header(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

/// Reads a device's description and picks the internet gateway service in it.
fn from_description(location: &str) -> Result<Gateway, String> {
    let xml = crate::backend::get(location)?
        .into_string()
        .map_err(|e| format!("{location}: {e}"))?;
    let services = services(&xml);
    // WANIPConnection is the common case; WANPPPConnection is what PPPoE routers expose.
    for wanted in ["WANIPConnection", "WANPPPConnection"] {
        if let Some((service, control)) = services
            .iter()
            .find(|(service, _)| service.contains(wanted))
        {
            return Ok(Gateway {
                location: location.to_string(),
                control: absolute(location, control),
                service: service.clone(),
            });
        }
    }
    Err("The router answered, but names no WAN connection service.".to_string())
}

/// Every `(serviceType, controlURL)` in a description document.
fn services(xml: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for block in xml.split("<service>").skip(1) {
        let Some(block) = block.split("</service>").next() else {
            continue;
        };
        let (Some(service), Some(control)) = (tag(block, "serviceType"), tag(block, "controlURL")) else {
            continue;
        };
        found.push((service, control));
    }
    found
}

/// The text of the first `<name>…</name>` in `xml`.
fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_string())
}

/// `control` as a full URL, against `location`'s host and scheme.
fn absolute(location: &str, control: &str) -> String {
    if control.starts_with("http://") || control.starts_with("https://") {
        return control.to_string();
    }
    let scheme = location.split("://").next().unwrap_or("http");
    let host = location
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or_default();
    let path = control.strip_prefix('/').unwrap_or(control);
    format!("{scheme}://{host}/{path}")
}

/// The router's address on the internet, as it sees it.
pub fn external_ip(gateway: &Gateway) -> Result<String, String> {
    let body = soap(gateway, "GetExternalIPAddress", "")?;
    tag(&body, "NewExternalIPAddress").ok_or_else(|| "The router named no address.".to_string())
}

/// Asks the router to send `external_port` (TCP) to `internal` at `internal_port`.
///
/// Some routers only take permanent mappings (`NewLeaseDuration` 0): those are tried when the
/// timed one is refused.
pub fn map(
    gateway: &Gateway,
    external_port: u16,
    internal: &str,
    internal_port: u16,
    lease: u32,
) -> Result<(), String> {
    for lease in [lease, 0] {
        let body = format!(
            "<NewRemoteHost></NewRemoteHost>\
             <NewExternalPort>{external_port}</NewExternalPort>\
             <NewProtocol>TCP</NewProtocol>\
             <NewInternalPort>{internal_port}</NewInternalPort>\
             <NewInternalClient>{internal}</NewInternalClient>\
             <NewEnabled>1</NewEnabled>\
             <NewPortMappingDescription>Horizon Offline Launcher</NewPortMappingDescription>\
             <NewLeaseDuration>{lease}</NewLeaseDuration>"
        );
        match soap(gateway, "AddPortMapping", &body) {
            Ok(_) => return Ok(()),
            // 725: this router only takes permanent leases, which the loop's second pass asks for.
            Err(e) if e.ends_with("(725)") => continue,
            Err(e) => return Err(e),
        }
    }
    Err("The router refused the mapping.".to_string())
}

/// The description of the mapping on `external_port`, when there is one.
pub fn mapping(gateway: &Gateway, external_port: u16) -> Option<String> {
    let body = soap(
        gateway,
        "GetSpecificPortMappingEntry",
        &format!(
            "<NewRemoteHost></NewRemoteHost>\
             <NewExternalPort>{external_port}</NewExternalPort>\
             <NewProtocol>TCP</NewProtocol>"
        ),
    )
    .ok()?;
    tag(&body, "NewPortMappingDescription")
}

/// Takes a mapping away.
pub fn unmap(gateway: &Gateway, external_port: u16) -> Result<(), String> {
    soap(
        gateway,
        "DeletePortMapping",
        &format!(
            "<NewRemoteHost></NewRemoteHost>\
             <NewExternalPort>{external_port}</NewExternalPort>\
             <NewProtocol>TCP</NewProtocol>"
        ),
    )
    .map(|_| ())
}

/// One SOAP action, and the body it answered with.
///
/// The `SOAPAction` header is quoted in UPnP's spec, and some routers (Huawei's home gateways,
/// for one) do not strip those quotes and answer "Invalid Action": the header is sent without
/// them, then with them for the routers that expect the spec.
fn soap(gateway: &Gateway, action: &str, arguments: &str) -> Result<String, String> {
    let envelope = format!(
        "<?xml version=\"1.0\"?>\r\n\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body><u:{action} xmlns:u=\"{service}\">{arguments}</u:{action}></s:Body></s:Envelope>",
        service = gateway.service
    );
    let mut last = String::new();
    for header in [
        format!("{}#{action}", gateway.service),
        format!("\"{}#{action}\"", gateway.service),
    ] {
        for attempt in 0..2 {
            let sent = ureq::post(&gateway.control)
                .set("Content-Type", "text/xml; charset=\"utf-8\"")
                .set("SOAPAction", &header)
                .timeout(Duration::from_secs(5))
                .send_string(&envelope);
            match sent {
                Ok(response) => {
                    return response
                        .into_string()
                        .map_err(|e| format!("The router's answer: {e}"));
                }
                Err(ureq::Error::Status(code, response)) => {
                    let body = response.into_string().unwrap_or_default();
                    last = fault(code, &body, action);
                    // 401 and 402 are what a misread action or argument list answers: the
                    // other header form may fare better, and these gateways are slow.
                    match (fault_code(&body), attempt) {
                        (Some(401), _) => break,
                        (Some(402 | 501), 0) => {
                            std::thread::sleep(Duration::from_millis(700));
                        }
                        _ => {}
                    }
                }
                Err(e) => return Err(format!("The router did not answer: {e}")),
            }
        }
    }
    Err(last)
}

/// The `errorCode` of a SOAP fault, if the body holds one.
fn fault_code(body: &str) -> Option<u16> {
    tag(body, "errorCode")?.parse().ok()
}

/// A SOAP fault as one line a player can act on: the code is kept (718 is a taken port, 725 a
/// router that only takes permanent mappings, 606 an action it doesn't offer).
fn fault(status: u16, body: &str, action: &str) -> String {
    let code = tag(body, "errorCode").unwrap_or_default();
    let reason = match code.as_str() {
        "718" => "That port is already forwarded by something else".to_string(),
        "725" => "This router only takes permanent mappings".to_string(),
        "606" => "This router does not offer port forwarding (UPnP is off?)".to_string(),
        "402" | "501" => "This router refused the request".to_string(),
        "714" => "This router does not allow that mapping".to_string(),
        _ => tag(body, "errorDescription")
            .unwrap_or_else(|| format!("The router answered {status}")),
    };
    match code.is_empty() {
        true => format!("{action}: {reason}"),
        false => format!("{action}: {reason} ({code})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESCRIPTION: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:InternetGatewayDevice:1</deviceType>
    <deviceList>
      <device>
        <deviceType>urn:schemas-upnp-org:device:WANDevice:1</deviceType>
        <deviceList>
          <device>
            <deviceType>urn:schemas-upnp-org:device:WANConnectionDevice:1</deviceType>
            <serviceList>
              <service>
                <serviceType>urn:schemas-upnp-org:service:WANPPPConnection:1</serviceType>
                <controlURL>/upnp/control/wanppp</controlURL>
              </service>
              <service>
                <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
                <controlURL>/ctl/IPConn</controlURL>
              </service>
            </serviceList>
          </device>
        </deviceList>
      </device>
    </deviceList>
  </device>
</root>"#;

    #[test]
    fn finds_the_internet_service_and_its_control() {
        let services = services(DESCRIPTION);
        assert_eq!(services.len(), 2);
        let (service, control) = services
            .iter()
            .find(|(service, _)| service.contains("WANIPConnection"))
            .expect("WANIPConnection is in the description");
        assert_eq!(service, "urn:schemas-upnp-org:service:WANIPConnection:1");
        assert_eq!(control, "/ctl/IPConn");
        assert_eq!(
            absolute("http://192.168.1.1:5000/rootDesc.xml", control),
            "http://192.168.1.1:5000/ctl/IPConn"
        );
    }

    #[test]
    fn reads_headers_whatever_their_case() {
        let answer = "HTTP/1.1 200 OK\r\nLOCATION: http://192.168.1.1:1900/x.xml\r\nST: upnp:rootdevice\r\n\r\n";
        assert_eq!(
            header(answer, "location").as_deref(),
            Some("http://192.168.1.1:1900/x.xml")
        );
        assert_eq!(header(answer, "st").as_deref(), Some("upnp:rootdevice"));
        assert_eq!(header(answer, "usn"), None);
    }

    #[test]
    fn faults_name_what_the_player_can_do() {
        let taken = "<s:Fault><detail><UPnPError><errorCode>718</errorCode>\
                     <errorDescription>ConflictInMappingEntry</errorDescription>\
                     </UPnPError></detail></s:Fault>";
        let message = fault(500, taken, "AddPortMapping");
        assert!(message.contains("already forwarded"), "{message}");
        assert!(message.ends_with("(718)"), "{message}");
        let permanent = "<errorCode>725</errorCode><errorDescription>OnlyPermanentLeasesSupported</errorDescription>";
        assert!(fault(500, permanent, "AddPortMapping").ends_with("(725)"));
    }

    #[test]
    fn reads_the_routers_address() {
        let body = "<NewExternalIPAddress>203.0.113.7</NewExternalIPAddress>";
        assert_eq!(tag(body, "NewExternalIPAddress").as_deref(), Some("203.0.113.7"));
    }
}
