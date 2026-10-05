//! Hálózati segédek: a kliens IP-címe (a proxy fejlécek csak helyi proxytól származva számítanak).

use axum::http::HeaderMap;
use std::net::IpAddr;

/// Kliens IP cím. A proxy fejléceket (Cloudflare tunnel) csak akkor vesszük figyelembe, ha a kérés helyi (loopback)
/// proxyról érkezett. Közvetlen eléréskor a fejléc hamisítható volna, ami kiütné az IP-alapú forgalomkorlátot.
pub fn client_ip(headers: &HeaderMap, peer: Option<IpAddr>) -> String {
    let remote = peer.map(unmap).unwrap_or(IpAddr::from([127, 0, 0, 1]));
    if remote.is_loopback() {
        let forwarded = headers
            .get("cf-connecting-ip")
            .or_else(|| headers.get("x-forwarded-for"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let candidate = forwarded.split(',').next().unwrap_or("").trim();
        if !candidate.is_empty() {
            return candidate.to_string();
        }
    }
    remote.to_string()
}

/// Az IPv4-be képzett IPv6 cím (::ffff:a.b.c.d) szokásos IPv4 alakja.
fn unmap(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(*k, v.parse().unwrap());
        }
        map
    }

    #[test]
    fn proxy_headers_count_only_from_loopback() {
        let h = headers(&[("cf-connecting-ip", "203.0.113.7")]);
        assert_eq!(client_ip(&h, Some("127.0.0.1".parse().unwrap())), "203.0.113.7");
        assert_eq!(client_ip(&h, Some("::1".parse().unwrap())), "203.0.113.7");
        assert_eq!(client_ip(&h, Some("198.51.100.4".parse().unwrap())), "198.51.100.4");
        let forwarded = headers(&[("x-forwarded-for", "203.0.113.9, 10.0.0.1")]);
        assert_eq!(client_ip(&forwarded, Some("127.0.0.1".parse().unwrap())), "203.0.113.9");
        assert_eq!(client_ip(&HeaderMap::new(), Some("127.0.0.1".parse().unwrap())), "127.0.0.1");
        assert_eq!(client_ip(&HeaderMap::new(), None), "127.0.0.1");
    }
}
