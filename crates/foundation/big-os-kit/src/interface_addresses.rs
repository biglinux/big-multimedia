//! Addresses of this computer's network interfaces.
use std::ffi::CStr;
use std::io;
use std::net::Ipv4Addr;

/// Each IPv4 address with the name of its interface, in the kernel's order.
///
/// # Errors
///
/// When `getifaddrs` fails.
pub fn ipv4_interface_addresses() -> io::Result<Vec<(String, Ipv4Addr)>> {
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `list` is a valid out-pointer; on success it owns a list that
    // `freeifaddrs` releases below.
    if unsafe { libc::getifaddrs(&raw mut list) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut addresses = Vec::new();
    let mut entry = list;
    while !entry.is_null() {
        // SAFETY: `entry` is a node of the list `getifaddrs` returned, not yet freed.
        let node = unsafe { &*entry };
        entry = node.ifa_next;
        if node.ifa_addr.is_null() {
            continue;
        }
        // SAFETY: `ifa_addr` is non-null and points to a sockaddr owned by the list.
        if i32::from(unsafe { (*node.ifa_addr).sa_family }) != libc::AF_INET {
            continue;
        }
        // SAFETY: an AF_INET address is a sockaddr_in; read unaligned because
        // the list types it as the less aligned sockaddr.
        let ipv4 = unsafe { node.ifa_addr.cast::<libc::sockaddr_in>().read_unaligned() };
        // SAFETY: `ifa_name` is a NUL-terminated string owned by the list.
        let name = unsafe { CStr::from_ptr(node.ifa_name) };
        addresses.push((
            name.to_string_lossy().into_owned(),
            Ipv4Addr::from(u32::from_be(ipv4.sin_addr.s_addr)),
        ));
    }
    // SAFETY: `list` came from `getifaddrs`, and nothing read from it is borrowed.
    unsafe { libc::freeifaddrs(list) };
    Ok(addresses)
}

#[cfg(test)]
mod tests {
    #[test]
    fn loopback_is_listed() {
        let addresses = super::ipv4_interface_addresses().unwrap();
        assert!(
            addresses
                .iter()
                .any(|(name, address)| name == "lo" && address.is_loopback()),
            "{addresses:?}"
        );
    }
}
