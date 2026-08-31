//! SSRF protection for outbound webhook URLs.
//!
//! Fail-closed: DNS resolution failure, missing host, non-http(s), or any
//! resolved address in a private / link-local / metadata range → reject.
//! Re-resolves at check time so callers can guard against DNS rebinding.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

use thiserror::Error;
use url::{Host, Url};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SsrfError {
    #[error("url must be an absolute http(s) URL")]
    BadUrl,
    #[error("url scheme must be http or https")]
    BadScheme,
    #[error("url must include a host")]
    MissingHost,
    #[error("DNS resolution failed")]
    ResolveFailed,
    #[error("destination resolves to a blocked address")]
    BlockedAddress,
}

/// Validate `raw_url` for outbound webhook delivery (SSRF fail-closed).
///
/// Resolves the hostname and rejects if any address is private, loopback,
/// link-local, or cloud metadata (`169.254.169.254`).
pub fn assert_url_safe(raw_url: &str) -> Result<(), SsrfError> {
    let parsed = Url::parse(raw_url).map_err(|_| SsrfError::BadUrl)?;
    assert_parsed_safe(&parsed)
}

fn assert_parsed_safe(parsed: &Url) -> Result<(), SsrfError> {
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return Err(SsrfError::BadScheme),
    }
    let host = parsed.host().ok_or(SsrfError::MissingHost)?;
    let port = parsed.port_or_known_default().unwrap_or(80);

    match host {
        Host::Ipv4(v4) => {
            if is_blocked_ip(IpAddr::V4(v4)) {
                return Err(SsrfError::BlockedAddress);
            }
            Ok(())
        }
        Host::Ipv6(v6) => {
            if is_blocked_ip(IpAddr::V6(v6)) {
                return Err(SsrfError::BlockedAddress);
            }
            Ok(())
        }
        Host::Domain(domain) => {
            // DNS re-resolution guard: resolve now; caller should call again
            // immediately before connect if needed.
            let addrs = (domain, port)
                .to_socket_addrs()
                .map_err(|_| SsrfError::ResolveFailed)?;
            let mut any = false;
            for addr in addrs {
                any = true;
                if is_blocked_ip(addr.ip()) {
                    return Err(SsrfError::BlockedAddress);
                }
            }
            if !any {
                return Err(SsrfError::ResolveFailed);
            }
            Ok(())
        }
    }
}

/// True when `ip` must not receive outbound webhook traffic.
pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => is_blocked_v6(v6),
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    // 0.0.0.0/8
    if octets[0] == 0 {
        return true;
    }
    // Loopback 127.0.0.0/8
    if ip.is_loopback() {
        return true;
    }
    // RFC1918 private
    if ip.is_private() {
        return true;
    }
    // Link-local 169.254.0.0/16 (includes metadata 169.254.169.254)
    if ip.is_link_local() {
        return true;
    }
    // Carrier-grade NAT 100.64.0.0/10
    if octets[0] == 100 && (octets[1] & 0xc0) == 64 {
        return true;
    }
    // Broadcast / multicast / unspecified
    if ip.is_broadcast() || ip.is_multicast() || ip.is_unspecified() {
        return true;
    }
    // Explicit metadata
    if octets == [169, 254, 169, 254] {
        return true;
    }
    false
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return true;
    }
    // Unique-local fc00::/7
    let segments = ip.segments();
    if (segments[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    // Link-local fe80::/10
    if (segments[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    // IPv4-mapped — check embedded v4
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_blocked_v4(v4);
    }
    // IPv4-compatible (deprecated) also embeds v4 in low 32 bits
    if let Some(v4) = ip.to_ipv4() {
        if segments[0] == 0
            && segments[1] == 0
            && segments[2] == 0
            && segments[3] == 0
            && segments[4] == 0
            && segments[5] == 0
        {
            return is_blocked_v4(v4);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_localhost_hostname() {
        assert_eq!(
            assert_url_safe("http://localhost/hook"),
            Err(SsrfError::BlockedAddress)
        );
    }

    #[test]
    fn rejects_loopback_v4() {
        assert_eq!(
            assert_url_safe("http://127.0.0.1/hook"),
            Err(SsrfError::BlockedAddress)
        );
        assert_eq!(
            assert_url_safe("http://127.1.2.3:8443/x"),
            Err(SsrfError::BlockedAddress)
        );
    }

    #[test]
    fn rejects_loopback_v6() {
        assert_eq!(
            assert_url_safe("http://[::1]/hook"),
            Err(SsrfError::BlockedAddress)
        );
    }

    #[test]
    fn rejects_rfc1918() {
        for url in [
            "http://10.0.0.1/h",
            "http://10.255.255.254/h",
            "http://192.168.1.1/h",
            "http://192.168.0.50/h",
            "http://172.16.0.1/h",
            "http://172.31.255.1/h",
        ] {
            assert_eq!(assert_url_safe(url), Err(SsrfError::BlockedAddress), "{url}");
        }
        // 172.32 is not RFC1918
        assert!(!is_blocked_ip(IpAddr::V4(Ipv4Addr::new(172, 32, 0, 1))));
        assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
        assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(172, 31, 255, 1))));
        assert!(!is_blocked_ip(IpAddr::V4(Ipv4Addr::new(172, 15, 0, 1))));
    }

    #[test]
    fn rejects_link_local_and_metadata() {
        assert_eq!(
            assert_url_safe("http://169.254.169.254/latest/meta-data"),
            Err(SsrfError::BlockedAddress)
        );
        assert_eq!(
            assert_url_safe("http://169.254.1.1/x"),
            Err(SsrfError::BlockedAddress)
        );
        assert!(is_blocked_ip(IpAddr::V6(
            "fe80::1".parse().expect("ip")
        )));
    }

    #[test]
    fn rejects_bad_scheme() {
        assert_eq!(
            assert_url_safe("ftp://example.com/x"),
            Err(SsrfError::BadScheme)
        );
        assert_eq!(assert_url_safe("not a url"), Err(SsrfError::BadUrl));
    }

    #[test]
    fn public_literal_ok() {
        // 8.8.8.8 is public; no DNS needed.
        assert_eq!(assert_url_safe("https://8.8.8.8/webhook"), Ok(()));
    }
}
