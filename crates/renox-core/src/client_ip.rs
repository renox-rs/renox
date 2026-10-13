//! The visitor's IP address, also behind a reverse proxy.
//!
//! Behind Caddy, nginx or a load balancer every connection comes from the
//! proxy, which passes the visitor's address in `X-Forwarded-For` or
//! `Forwarded`. Those headers are believed only from the addresses in
//! `TRUSTED_PROXIES`, since anyone can send them:
//!
//! ```text
//! TRUSTED_PROXIES=127.0.0.1,10.0.0.0/8   # Caddy on the same host, a private network
//! TRUSTED_PROXIES=*                      # whoever connects, e.g. a platform's load balancer
//! ```
//!
//! Rate limits, the login lock and [`ClientIp`] all use the same address.
//!
//! ```
//! # use renox::prelude::*;
//! async fn whoami(ClientIp(ip): ClientIp) -> String {
//!     ip.map_or("unknown".into(), |ip| ip.to_string())
//! }
//! ```

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use anyhow::{Context, bail};
use axum::extract::{ConnectInfo, FromRequestParts, Request};
use axum::http::HeaderMap;
use axum::http::request::Parts;

/// The client's IP address, when the server knows it: the connection's
/// address, or the one a trusted proxy forwarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub Option<IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(match parts.extensions.get::<ClientIp>() {
            Some(ip) => *ip,
            None => Self(peer(&parts.extensions)),
        })
    }
}

impl ClientIp {
    /// The address of a request that went through the app's middleware.
    pub(crate) fn of(req: &Request) -> Option<IpAddr> {
        match req.extensions().get::<ClientIp>() {
            Some(ip) => ip.0,
            None => peer(req.extensions()),
        }
    }
}

/// Proxies whose `X-Forwarded-For` and `Forwarded` headers are believed,
/// from `TRUSTED_PROXIES`: addresses and CIDR ranges separated by commas, or
/// `*` to believe whoever connects, for the last hop only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies {
    any: bool,
    nets: Vec<(IpAddr, u8)>,
}

impl TrustedProxies {
    /// Parses a `TRUSTED_PROXIES` list; fails on a bad address or prefix, or `*`
    /// mixed with addresses.
    pub fn parse(list: &str) -> crate::Result<Self> {
        Ok(Self::read(list)?)
    }

    pub(crate) fn read(list: &str) -> anyhow::Result<Self> {
        let mut proxies = Self::default();
        for item in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if item == "*" {
                proxies.any = true;
                continue;
            }
            let (addr, bits) = match item.split_once('/') {
                Some((addr, bits)) => (addr, Some(bits)),
                None => (item, None),
            };
            let addr: IpAddr = addr
                .parse()
                .with_context(|| format!("TRUSTED_PROXIES: `{item}` is not an IP address"))?;
            let max = if addr.is_ipv4() { 32 } else { 128 };
            let bits = match bits {
                Some(bits) => bits
                    .parse::<u8>()
                    .ok()
                    .filter(|b| *b <= max)
                    .with_context(|| format!("TRUSTED_PROXIES: bad prefix length in `{item}`"))?,
                None => max,
            };
            proxies.nets.push((addr.to_canonical(), bits));
        }
        if proxies.any && !proxies.nets.is_empty() {
            bail!("TRUSTED_PROXIES: `*` already trusts every address");
        }
        Ok(proxies)
    }

    /// Whether `ip` is a trusted proxy.
    pub fn contains(&self, ip: IpAddr) -> bool {
        self.any
            || self
                .nets
                .iter()
                .any(|(net, bits)| in_net(ip.to_canonical(), *net, *bits))
    }

    fn is_empty(&self) -> bool {
        !self.any && self.nets.is_empty()
    }
}

fn in_net(ip: IpAddr, net: IpAddr, bits: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(ip), IpAddr::V4(net)) => {
            let mask = u32::MAX.checked_shl(32 - u32::from(bits)).unwrap_or(0);
            u32::from(ip) & mask == u32::from(net) & mask
        }
        (IpAddr::V6(ip), IpAddr::V6(net)) => {
            let mask = u128::MAX.checked_shl(128 - u32::from(bits)).unwrap_or(0);
            u128::from(ip) & mask == u128::from(net) & mask
        }
        _ => false,
    }
}

fn peer(extensions: &axum::http::Extensions) -> Option<IpAddr> {
    extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_canonical())
}

/// The client's address: the connection's, unless it is a trusted proxy.
/// Then the forwarded chain is walked from the right (the hop nearest to
/// us), skipping trusted proxies, since only the entries they appended can
/// be believed.
pub(crate) fn resolve(req: &Request, trusted: &TrustedProxies) -> ClientIp {
    let Some(peer) = peer(req.extensions()) else {
        return ClientIp(None);
    };
    if trusted.is_empty() || !trusted.contains(peer) {
        return ClientIp(Some(peer));
    }
    let mut client = peer;
    for hop in forwarded_chain(req.headers()).iter().rev() {
        let Some(ip) = hop else {
            // Garbage the previous hop didn't vouch for; stop at that hop.
            break;
        };
        client = *ip;
        // `*` vouches for the connecting proxy only: one hop.
        if trusted.any || !trusted.contains(client) {
            break;
        }
    }
    ClientIp(Some(client))
}

/// Addresses in `X-Forwarded-For`, or else in `Forwarded`, left to right;
/// `None` where an entry is not an address (`unknown`, an obfuscated name).
fn forwarded_chain(headers: &HeaderMap) -> Vec<Option<IpAddr>> {
    let xff: Vec<Option<IpAddr>> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(parse_node)
        .collect();
    if !xff.is_empty() {
        return xff;
    }
    headers
        .get_all("forwarded")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|element| {
            element.split(';').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                key.trim()
                    .eq_ignore_ascii_case("for")
                    .then(|| parse_node(value))
            })
        })
        .collect()
}

/// `1.2.3.4`, `1.2.3.4:80`, `"[2001:db8::1]:4711"` or `2001:db8::1`.
fn parse_node(node: &str) -> Option<IpAddr> {
    let node = node.trim().trim_matches('"');
    if let Ok(ip) = node.parse::<IpAddr>() {
        return Some(ip.to_canonical());
    }
    if let Ok(addr) = node.parse::<SocketAddr>() {
        return Some(addr.ip().to_canonical());
    }
    node.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(|ip| ip.parse::<IpAddr>().ok())
        .map(|ip| ip.to_canonical())
}

#[cfg(test)]
mod tests {
    use axum::body::Body;

    use super::*;

    fn request(peer: &str, headers: &[(&str, &str)]) -> Request {
        let mut builder = Request::builder().uri("/");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let mut req = builder.body(Body::empty()).unwrap();
        let addr: SocketAddr = format!("{peer}:5555").parse().unwrap();
        req.extensions_mut().insert(ConnectInfo(addr));
        req
    }

    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn parses_addresses_ranges_and_any() {
        let proxies = TrustedProxies::parse("127.0.0.1, 10.0.0.0/8,fd00::/8").unwrap();
        assert!(proxies.contains("10.9.8.7".parse().unwrap()));
        assert!(proxies.contains("::ffff:127.0.0.1".parse().unwrap()));
        assert!(proxies.contains("fd12::1".parse().unwrap()));
        assert!(!proxies.contains("11.0.0.1".parse().unwrap()));
        assert!(
            TrustedProxies::parse("*")
                .unwrap()
                .contains("8.8.8.8".parse().unwrap())
        );
        assert!(TrustedProxies::parse("").unwrap().is_empty());
        assert!(TrustedProxies::parse("10.0.0.0/33").is_err());
        assert!(TrustedProxies::parse("proxy.local").is_err());
        let err = TrustedProxies::parse("*, 10.0.0.1").unwrap_err();
        assert!(
            format!("{err:?}").contains("`*` already trusts every address"),
            "{err:?}"
        );
    }

    /// Outside the app's middleware (no resolved address yet), the
    /// connection's address is the client's.
    #[tokio::test]
    async fn without_the_middleware_the_peer_is_the_client() {
        let req = request("203.0.113.9", &[("x-forwarded-for", "1.1.1.1")]);
        assert_eq!(ClientIp::of(&req), ip("203.0.113.9"));
        let (mut parts, _) = req.into_parts();
        let found = ClientIp::from_request_parts(&mut parts, &()).await.unwrap();
        assert_eq!(found.0, ip("203.0.113.9"));
    }

    #[test]
    fn ignores_forwarded_headers_from_untrusted_peers() {
        let trusted = TrustedProxies::parse("10.0.0.1").unwrap();
        let req = request("203.0.113.9", &[("x-forwarded-for", "1.1.1.1")]);
        assert_eq!(resolve(&req, &trusted).0, ip("203.0.113.9"));
        let req = request("10.0.0.1", &[("x-forwarded-for", "1.1.1.1")]);
        assert_eq!(resolve(&req, &TrustedProxies::default()).0, ip("10.0.0.1"));
    }

    #[test]
    fn takes_the_rightmost_untrusted_hop() {
        let trusted = TrustedProxies::parse("10.0.0.0/8").unwrap();
        // The visitor made up 6.6.6.6; the proxy appended the real 1.2.3.4.
        let req = request(
            "10.0.0.1",
            &[("x-forwarded-for", "6.6.6.6, 1.2.3.4, 10.0.0.2")],
        );
        assert_eq!(resolve(&req, &trusted).0, ip("1.2.3.4"));
        let req = request("10.0.0.1", &[("x-forwarded-for", "10.0.0.3")]);
        assert_eq!(resolve(&req, &trusted).0, ip("10.0.0.3"));
        let req = request("10.0.0.1", &[("x-forwarded-for", "unknown")]);
        assert_eq!(resolve(&req, &trusted).0, ip("10.0.0.1"));
    }

    #[test]
    fn reads_the_forwarded_header() {
        let trusted = TrustedProxies::parse("*").unwrap();
        let req = request(
            "10.0.0.1",
            &[(
                "forwarded",
                r#"for=192.0.2.60;proto=https, for="[2001:db8::1]:4711""#,
            )],
        );
        assert_eq!(resolve(&req, &trusted).0, ip("2001:db8::1"));
        // `*` reads one hop, so a made-up first entry doesn't count.
        let req = request("10.0.0.1", &[("x-forwarded-for", "6.6.6.6, 1.2.3.4")]);
        assert_eq!(resolve(&req, &trusted).0, ip("1.2.3.4"));
    }
}
