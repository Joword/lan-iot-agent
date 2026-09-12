//! Probe the LAN for Companion HTTP already online (PC / phone / robot).
//!
//! `kind=robot` on port 9879 is a product path for a chassis already on the
//! LAN. `kind=chip` on port 9878 is an R&D firmware hook, not a home device
//! class. Identity: `lan_iot: true` or `service` `companion.*` / `lan-iot-*`.
//! Adopt writes the Companion registry.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::adapters::companion::CompanionDevice;

const DEFAULT_PORTS: &[u16] = &[9876, 9877, 9878, 9879];
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanEndpoint {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    #[serde(default)]
    pub commands: Vec<String>,
    pub adopted: bool,
}

/// Scan loopback, this host's /24, and `LAN_SCAN_HOSTS` (comma-separated).
pub async fn scan(adopted: &[CompanionDevice]) -> Vec<LanEndpoint> {
    let client = Client::builder()
        .timeout(PROBE_TIMEOUT)
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .build()
        .unwrap_or_else(|_| Client::new());

    let mut urls = Vec::new();
    for host in scan_hosts() {
        for port in scan_ports() {
            urls.push(format!("http://{host}:{port}"));
        }
    }
    urls.sort();
    urls.dedup();

    let sem = Arc::new(Semaphore::new(64));
    let mut set = JoinSet::new();
    for base in urls {
        let client = client.clone();
        let sem = Arc::clone(&sem);
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok()?;
            probe(&client, &base).await
        });
    }

    let adopted_urls: HashSet<String> = adopted
        .iter()
        .map(|c| c.base_url.trim_end_matches('/').to_string())
        .collect();
    let adopted_ids: HashSet<String> = adopted.iter().map(|c| c.id.clone()).collect();

    let mut found = Vec::new();
    while let Some(join) = set.join_next().await {
        if let Ok(Some(mut ep)) = join {
            ep.adopted = adopted_urls.contains(ep.base_url.trim_end_matches('/'))
                || adopted_ids.contains(&ep.id);
            found.push(ep);
        }
    }
    found.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.id.cmp(&b.id)));
    found
}

/// GET `{base}/health` (and `/` as fallback) and parse a LanIoT identity.
pub async fn probe(client: &Client, base: &str) -> Option<LanEndpoint> {
    let base = base.trim_end_matches('/');
    for path in ["/health", "/"] {
        let url = format!("{base}{path}");
        let resp = client.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            continue;
        }
        let body: Value = resp.json().await.ok()?;
        if let Some(mut ep) = parse_health(base, &body) {
            if ep.base_url.is_empty() {
                ep.base_url = base.to_string();
            }
            return Some(ep);
        }
    }
    None
}

pub fn parse_health(base: &str, body: &Value) -> Option<LanEndpoint> {
    if !body.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let service = body
        .get("service")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let lan_iot = body
        .get("lan_iot")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !lan_iot
        && !service.starts_with("companion.")
        && !service.starts_with("lan-iot-")
    {
        return None;
    }

    let port = base
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(0);
    let kind = body
        .get("kind")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| infer_kind(&service, port));
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_id(&kind, base));
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_name(&kind, &id));
    let commands = body
        .get("commands")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| default_commands(&kind));

    Some(LanEndpoint {
        id,
        name,
        kind,
        base_url: base.trim_end_matches('/').to_string(),
        commands,
        adopted: false,
    })
}

fn infer_kind(service: &str, port: u16) -> String {
    if service.contains("android") || port == 9877 {
        "phone".into()
    } else if service.contains("robot") || port == 9879 {
        "robot".into()
    } else if service.contains("chip") || port == 9878 {
        "chip".into()
    } else {
        "pc".into()
    }
}

fn default_id(kind: &str, base: &str) -> String {
    let host = base
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .replace(['.', ':'], "_");
    format!("{kind}.{host}")
}

fn default_name(kind: &str, id: &str) -> String {
    match kind {
        "chip" => format!("R&D chip hook ({id})"),
        "robot" => format!("Robot ({id})"),
        "phone" => format!("Phone ({id})"),
        _ => format!("PC ({id})"),
    }
}

fn default_commands(kind: &str) -> Vec<String> {
    match kind {
        "chip" => vec!["ping".into(), "turn_on".into(), "turn_off".into()],
        "robot" => vec!["ping".into(), "stop".into(), "dock".into(), "start".into()],
        _ => vec!["ping".into(), "notify".into(), "lock".into()],
    }
}

fn scan_ports() -> Vec<u16> {
    if let Ok(raw) = std::env::var("LAN_SCAN_PORTS") {
        let parsed: Vec<u16> = raw
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if !parsed.is_empty() {
            return parsed;
        }
    }
    DEFAULT_PORTS.to_vec()
}

fn scan_hosts() -> Vec<String> {
    let mut hosts: HashSet<String> = HashSet::new();
    hosts.insert("127.0.0.1".into());
    hosts.insert("localhost".into());
    hosts.insert("host.docker.internal".into());

    if let Some(ip) = primary_ipv4() {
        hosts.insert(ip.to_string());
        if ip.is_private() && !ip.is_loopback() {
            let oct = ip.octets();
            for last in 1..=254u8 {
                hosts.insert(Ipv4Addr::new(oct[0], oct[1], oct[2], last).to_string());
            }
        }
    }

    if let Ok(extra) = std::env::var("LAN_SCAN_HOSTS") {
        for h in extra.split(',') {
            let h = h.trim();
            if !h.is_empty() {
                hosts.insert(h.to_string());
            }
        }
    }
    hosts.into_iter().collect()
}

fn primary_ipv4() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect(SocketAddr::from((Ipv4Addr::new(8, 8, 8, 8), 80)))
        .ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(v) => Some(v),
        IpAddr::V6(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_windows_health() {
        let ep = parse_health(
            "http://192.168.1.20:9876",
            &json!({
                "ok": true,
                "lan_iot": true,
                "kind": "pc",
                "id": "companion.office",
                "name": "Office PC",
                "service": "companion.windows",
            }),
        )
        .expect("pc");
        assert_eq!(ep.kind, "pc");
        assert_eq!(ep.id, "companion.office");
        assert!(ep.commands.contains(&"lock".into()));
    }

    #[test]
    fn parse_chip_health() {
        let ep = parse_health(
            "http://192.168.1.40:9878",
            &json!({
                "ok": true,
                "lan_iot": true,
                "kind": "chip",
                "id": "chip.esp32_desk",
                "name": "Desk ESP32",
                "service": "lan-iot-chip",
                "commands": ["ping", "turn_on", "turn_off"],
            }),
        )
        .expect("chip");
        assert_eq!(ep.kind, "chip");
        assert_eq!(ep.id, "chip.esp32_desk");
    }

    #[test]
    fn parse_robot_health() {
        let ep = parse_health(
            "http://192.168.1.50:9879",
            &json!({
                "ok": true,
                "lan_iot": true,
                "kind": "robot",
                "id": "robot.lan_demo",
                "name": "Hall robot",
                "service": "lan-iot-robot",
                "commands": ["ping", "stop", "dock", "start"],
            }),
        )
        .expect("robot");
        assert_eq!(ep.kind, "robot");
        assert_eq!(ep.id, "robot.lan_demo");
        assert!(ep.commands.contains(&"stop".into()));
        assert!(ep.commands.contains(&"dock".into()));
    }

    #[test]
    fn ignore_random_ok_json() {
        assert!(parse_health("http://127.0.0.1:80", &json!({ "ok": true, "server": "nginx" })).is_none());
    }
}
