use std::{
    fmt, io,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
};

use crate::{Error, Result};

const SCHEME: &str = "http://";

const HTTP_PORT: u16 = 80;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Located {
    host: Box<str>,
    port: u16,
    path: Box<str>,
}

impl Located {
    pub fn parse(address: &str) -> Result<Self> {
        let address = address.trim();
        let rest = address
            .get(..SCHEME.len())
            .filter(|scheme| scheme.eq_ignore_ascii_case(SCHEME))
            .and_then(|_| address.get(SCHEME.len()..))
            .ok_or(Error::UnreadableAddress)?;
        let (authority, path) = match rest.find('/') {
            Some(at) => rest.split_at(at),
            None => (rest, "/"),
        };
        let authority = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let (host, port) = host_and_port(authority)?;
        if host.is_empty() {
            return Err(Error::UnreadableAddress);
        }

        Ok(Self {
            host: host.into(),
            port,
            path: path.into(),
        })
    }

    pub fn join(&self, reference: &str) -> Result<Self> {
        let reference = reference.trim();
        if reference.is_empty() {
            return Err(Error::UnreadableAddress);
        }
        if reference
            .get(..SCHEME.len())
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case(SCHEME))
        {
            return Self::parse(reference);
        }
        let path = if reference.starts_with('/') {
            reference.to_owned()
        } else {
            let folder = self
                .path
                .rfind('/')
                .and_then(|at| self.path.get(..=at))
                .unwrap_or("/");
            format!("{folder}{reference}")
        };

        Ok(Self {
            path: path.into(),
            ..self.clone()
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn is_on_host_of(&self, other: &Self) -> bool {
        self.host.eq_ignore_ascii_case(&other.host)
    }

    pub(crate) fn is_at(&self, ip: IpAddr) -> bool {
        let host = self.host.split('%').next().unwrap_or_default();
        host.parse::<IpAddr>()
            .is_ok_and(|named| named.to_canonical() == ip.to_canonical())
    }

    pub fn authority(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn reached_at(&self) -> io::Result<SocketAddr> {
        (self.host.as_ref(), self.port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::from(io::ErrorKind::AddrNotAvailable))
    }
}

impl fmt::Display for Located {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{SCHEME}{}{}", self.authority(), self.path)
    }
}

fn host_and_port(authority: &str) -> Result<(&str, u16)> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, after) = bracketed.split_once(']').ok_or(Error::UnreadableAddress)?;
        let port = match after.strip_prefix(':') {
            Some(port) => port.parse().map_err(|_| Error::UnreadableAddress)?,
            None if after.is_empty() => HTTP_PORT,
            None => return Err(Error::UnreadableAddress),
        };
        return Ok((host, port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Ok((host, port.parse().map_err(|_| Error::UnreadableAddress)?)),
        None => Ok((authority, HTTP_PORT)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_read_into_host_port_and_path() {
        let plain = Located::parse("http://192.168.1.20:49152/description.xml").expect("reads");
        let bare = Located::parse("HTTP://renderer.local").expect("reads");
        let six = Located::parse("http://[fe80::1]:8080/dev").expect("reads");

        assert_eq!(plain.host(), "192.168.1.20");
        assert_eq!(plain.port(), 49152);
        assert_eq!(plain.path(), "/description.xml");
        assert_eq!(bare.port(), 80);
        assert_eq!(bare.path(), "/");
        assert_eq!(six.host(), "fe80::1");
        assert_eq!(six.authority(), "[fe80::1]:8080");
        assert!(Located::parse("https://secure/").is_err());
        assert!(Located::parse("http://host:port/").is_err());
    }

    #[test]
    fn an_address_is_at_an_ip_only_when_it_names_that_ip_and_not_a_host_that_resolves_to_it() {
        let ip: IpAddr = "192.168.1.20".parse().expect("an ip");
        let six: IpAddr = "fe80::1".parse().expect("an ip");

        assert!(
            Located::parse("http://192.168.1.20:49152/d.xml")
                .expect("reads")
                .is_at(ip)
        );
        assert!(
            !Located::parse("http://192.168.1.21/d.xml")
                .expect("reads")
                .is_at(ip)
        );
        assert!(
            !Located::parse("http://renderer.local/d.xml")
                .expect("reads")
                .is_at(ip)
        );
        assert!(
            Located::parse("http://[fe80::1%eth0]:80/d.xml")
                .expect("reads")
                .is_at(six)
        );
        assert!(
            !Located::parse("http://127.0.0.1/d.xml")
                .expect("reads")
                .is_at(ip)
        );
    }

    #[test]
    fn two_addresses_share_a_host_whatever_their_port_path_or_case() {
        let one = Located::parse("http://Renderer.local:80/a").expect("reads");
        let two = Located::parse("http://renderer.LOCAL:1400/b").expect("reads");
        let other = Located::parse("http://renderer.local.evil:80/a").expect("reads");

        assert!(one.is_on_host_of(&two));
        assert!(!one.is_on_host_of(&other));
    }

    #[test]
    fn a_control_address_is_read_against_the_description_it_came_in() {
        let base = Located::parse("http://10.0.0.5:1400/xml/device.xml").expect("reads");

        assert_eq!(
            base.join("/MediaRenderer/AVTransport/Control")
                .expect("joins")
                .to_string(),
            "http://10.0.0.5:1400/MediaRenderer/AVTransport/Control"
        );
        assert_eq!(
            base.join("control/av").expect("joins").to_string(),
            "http://10.0.0.5:1400/xml/control/av"
        );
        assert_eq!(
            base.join("http://10.0.0.6:80/x").expect("joins").host(),
            "10.0.0.6"
        );
    }
}
