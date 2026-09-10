use big_app_kit::subprocess::BigSubprocessSpec;
use log::debug;

use crate::cast::chromecast::DiscoveredCastDevice;

/// Fallback discovery using avahi-browse (available on most Linux desktops).
pub(in crate::cast::chromecast) fn avahi_browse_chromecasts() -> Vec<DiscoveredCastDevice> {
    let output = BigSubprocessSpec::builder()
        .program("avahi-browse")
        .args(["-r", "-t", "-p", "_googlecast._tcp"])
        .build()
        .run();

    let Ok(output) = output else {
        debug!("avahi-browse not available");
        return vec![];
    };
    if !output.status.success() {
        debug!(
            "avahi-browse failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return vec![];
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_avahi_output(&stdout)
}

pub(super) fn parse_avahi_output(output: &str) -> Vec<DiscoveredCastDevice> {
    let mut devices = Vec::new();

    for line in output.lines() {
        let fields: Vec<&str> = line.split(';').collect();
        if fields.len() < 10 || fields[0] != "=" {
            continue;
        }
        if fields[2] == "IPv6" {
            continue;
        }
        let address = fields[7];
        let port: u16 = fields[8].parse().unwrap_or(8009);
        let txt = fields[9..].join(";");
        let name = extract_avahi_txt_value(&txt, "fn").unwrap_or_else(|| fields[3].to_string());
        let model = extract_avahi_txt_value(&txt, "md").unwrap_or_default();

        if !address.is_empty() {
            devices.push(DiscoveredCastDevice {
                name,
                host: address.to_string(),
                port,
                model,
            });
        }
    }

    devices
}

pub(super) fn extract_avahi_txt_value(txt: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    for part in txt.split("\" ") {
        let part = part.trim_matches('"');
        if let Some(val) = part.strip_prefix(&needle) {
            return Some(val.to_string());
        }
    }
    None
}
