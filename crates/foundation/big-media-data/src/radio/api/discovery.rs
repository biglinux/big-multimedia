//! Radio Browser API server discovery via DNS round-robin + per-call rotation.
//! Falls back to a hard-coded server list if DNS resolution fails.

use big_os_kit::http_client::http_get_fast;
use log::{debug, info, warn};
use std::net::ToSocketAddrs;

const DNS_DISCOVERY_DOMAIN: &str = "all.api.radio-browser.info";
const DEFAULT_SERVERS: &[&str] = &[
    "de2.api.radio-browser.info",
    "nl1.api.radio-browser.info",
    "at1.api.radio-browser.info",
    "de1.api.radio-browser.info",
];
const SETTINGS_KEY_SERVERS: &str = "radio-api-servers";

fn discover_servers_dns() -> Vec<String> {
    let addrs = match (DNS_DISCOVERY_DOMAIN, 443).to_socket_addrs() {
        Ok(addrs) => addrs,
        Err(error) => {
            warn!("[radio] DNS discovery failed for {DNS_DISCOVERY_DOMAIN}: {error}");
            return Vec::new();
        }
    };

    let mut seen = std::collections::HashSet::new();
    let mut servers = Vec::new();

    for addr in addrs {
        let ip = addr.ip();
        match dns_lookup::lookup_addr(&ip) {
            Ok(hostname) => {
                let hostname = hostname.trim_end_matches('.').to_string();
                if hostname.contains("api.radio-browser.info") && seen.insert(hostname.clone()) {
                    debug!("[radio] discovered server: {hostname} ({ip})");
                    servers.push(hostname);
                }
            }
            Err(error) => {
                debug!("[radio] reverse DNS failed for {ip}: {error}, using IP directly");
                let ip_str = ip.to_string();
                if seen.insert(ip_str.clone()) {
                    servers.push(ip_str);
                }
            }
        }
    }

    if servers.is_empty() {
        warn!("[radio] DNS discovery returned no servers, using defaults");
    } else {
        info!("[radio] discovered {} servers via DNS", servers.len());
    }

    servers
}

pub fn api_servers() -> Vec<String> {
    let discovered = discover_servers_dns();
    let servers = if discovered.is_empty() {
        DEFAULT_SERVERS
            .iter()
            .map(|server| (*server).to_string())
            .collect()
    } else {
        discovered
    };
    rotate_servers(servers)
}

/// The user's raw comma-separated server override (empty = DNS discovery). Cheap
/// settings read — safe on the main thread before spawning the blocking resolve.
pub fn settings_servers_raw(settings: &crate::settings::Settings) -> String {
    settings.string(SETTINGS_KEY_SERVERS)
}

/// Resolve the API server list from the raw override. Falls back to blocking DNS
/// discovery when no override is set — call this OFF the main thread.
pub fn api_servers_from_raw(raw: &str) -> Vec<String> {
    let configured: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|server| !server.is_empty())
        .map(String::from)
        .collect();
    if configured.is_empty() {
        api_servers()
    } else {
        rotate_servers(configured)
    }
}

fn rotate_servers(mut servers: Vec<String>) -> Vec<String> {
    if servers.len() <= 1 {
        return servers;
    }
    let offset = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as usize);
    let len = servers.len();
    servers.rotate_left(offset % len);
    servers
}

pub fn radio_browser_get_with_fallback(path: &str, servers: &[String]) -> Option<String> {
    for server in servers {
        let url = format!("https://{server}{path}");
        debug!("[radio] trying {url}");
        match http_get_fast(&url) {
            Some(body) => return Some(body),
            None => debug!("[radio] server {server} failed, trying next"),
        }
    }
    warn!("[radio] all servers failed for path: {path}");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_servers_not_empty() {
        assert!(!DEFAULT_SERVERS.is_empty());
    }

    #[test]
    fn default_servers_all_valid_hostnames() {
        for server in DEFAULT_SERVERS {
            assert!(
                server.contains("api.radio-browser.info"),
                "invalid server: {server}"
            );
            assert!(
                !server.starts_with("https://"),
                "server should be hostname only: {server}"
            );
        }
    }

    // DNS discovery calls libc resolver APIs; ordinary tests cover the system
    // boundary while Miri stays focused on pure server-list logic.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn api_servers_returns_non_empty() {
        assert!(
            !api_servers().is_empty(),
            "api_servers should never return empty"
        );
    }

    #[test]
    fn rotate_servers_preserves_elements() {
        let original = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let rotated = rotate_servers(original.clone());
        assert_eq!(rotated.len(), 3);
        assert!(rotated.contains(&"a".to_string()));
        assert!(rotated.contains(&"b".to_string()));
        assert!(rotated.contains(&"c".to_string()));
    }

    #[test]
    fn rotate_single_server_unchanged() {
        assert_eq!(rotate_servers(vec!["only".to_string()]), vec!["only"]);
    }

    #[test]
    fn rotate_empty_list_unchanged() {
        assert!(rotate_servers(Vec::new()).is_empty());
    }

    #[test]
    fn settings_key_defined() {
        assert_eq!(SETTINGS_KEY_SERVERS, "radio-api-servers");
    }

    // DNS discovery calls libc resolver APIs; ordinary tests cover the system
    // boundary while Miri stays focused on pure server-list logic.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn dns_discovery_returns_valid_servers() {
        let servers = discover_servers_dns();
        for server in &servers {
            assert!(
                !server.starts_with("https://"),
                "server should be hostname, not URL: {server}"
            );
            assert!(
                !server.starts_with("http://"),
                "server should be hostname, not URL: {server}"
            );
        }
    }
}
