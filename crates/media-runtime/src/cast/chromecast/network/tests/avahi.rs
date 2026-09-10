use super::super::avahi::{extract_avahi_txt_value, parse_avahi_output};
use crate::cast::chromecast::DiscoveredCastDevice;

#[test]
fn test_extract_avahi_txt_value_found() {
    let txt = r#""fn=Living Room" "md=Chromecast""#;
    assert_eq!(
        extract_avahi_txt_value(txt, "fn"),
        Some("Living Room".into())
    );
    assert_eq!(
        extract_avahi_txt_value(txt, "md"),
        Some("Chromecast".into())
    );
}

#[test]
fn test_extract_avahi_txt_value_not_found() {
    let txt = r#""fn=Kitchen" "md=Chromecast""#;
    assert_eq!(extract_avahi_txt_value(txt, "id"), None);
}

#[test]
fn test_extract_avahi_txt_value_empty_input() {
    assert_eq!(extract_avahi_txt_value("", "fn"), None);
}

#[test]
fn test_extract_avahi_txt_value_no_quotes() {
    assert_eq!(
        extract_avahi_txt_value("fn=Office", "fn"),
        Some("Office".into())
    );
}

#[test]
fn test_extract_avahi_txt_value_value_with_equals() {
    let txt = r#""fn=TV=Main" "md=Chromecast""#;
    assert_eq!(extract_avahi_txt_value(txt, "fn"), Some("TV=Main".into()));
}

#[test]
fn test_parse_avahi_output_single_device() {
    let output = "=;eth0;IPv4;Chromecast;_googlecast._tcp;local;cc.local;192.168.1.10;8009;\"fn=Living Room\" \"md=Chromecast\"\n";
    let devices = parse_avahi_output(output);
    assert_eq!(devices.len(), 1);
    assert_eq!(
        devices[0],
        DiscoveredCastDevice {
            name: "Living Room".into(),
            host: "192.168.1.10".into(),
            port: 8009,
            model: "Chromecast".into(),
        }
    );
}

#[test]
fn test_parse_avahi_output_skips_ipv6() {
    let output =
        "=;eth0;IPv6;Chromecast;_googlecast._tcp;local;cc.local;fe80::1;8009;\"fn=Room\"\n";
    assert!(parse_avahi_output(output).is_empty());
}

#[test]
fn test_parse_avahi_output_skips_non_resolved() {
    assert!(parse_avahi_output("+;eth0;IPv4;Chromecast;_googlecast._tcp;local\n").is_empty());
}

#[test]
fn test_parse_avahi_output_skips_short_lines() {
    assert!(parse_avahi_output("=;eth0;IPv4;name;type;domain;host\n").is_empty());
}

#[test]
fn test_parse_avahi_output_multiple_devices() {
    let output = "\
=;eth0;IPv4;CC1;_googlecast._tcp;local;cc1.local;192.168.1.10;8009;\"fn=Kitchen\" \"md=Chromecast\"
=;eth0;IPv4;CC2;_googlecast._tcp;local;cc2.local;192.168.1.11;8009;\"fn=Bedroom\" \"md=Audio\"
";
    let devices = parse_avahi_output(output);
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].name, "Kitchen");
    assert_eq!(devices[1].name, "Bedroom");
    assert_eq!(devices[1].model, "Audio");
}

#[test]
fn test_parse_avahi_output_empty_input() {
    assert!(parse_avahi_output("").is_empty());
}

#[test]
fn test_parse_avahi_output_missing_fn_uses_name_field() {
    let output = "=;eth0;IPv4;My-Device;_googlecast._tcp;local;dev.local;10.0.0.5;8009;\"\"\n";
    let devices = parse_avahi_output(output);
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name, "My-Device");
}

#[test]
fn test_parse_avahi_output_custom_port() {
    let output = "=;eth0;IPv4;CC;_googlecast._tcp;local;cc.local;10.0.0.1;9999;\"fn=TV\"\n";
    let devices = parse_avahi_output(output);
    assert_eq!(devices[0].port, 9999);
}

#[test]
fn test_parse_avahi_output_invalid_port_defaults_8009() {
    let output = "=;eth0;IPv4;CC;_googlecast._tcp;local;cc.local;10.0.0.1;bad;\"fn=TV\"\n";
    let devices = parse_avahi_output(output);
    assert_eq!(devices[0].port, 8009);
}
