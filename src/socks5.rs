use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// SOCKS5 proxy configuration.
/// Holds the proxy address and optional credentials.
#[derive(Clone, Debug)]
pub struct Socks5Proxy {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Socks5Proxy {
    /// Parses a SOCKS5 proxy URL and extracts connection info.
    /// Supported formats:
    ///   socks5://host:port
    ///   socks5://user:pass@host:port
    ///   host:port (the socks5:// prefix is optional)
    pub fn parse(url: &str) -> Result<Self, String> {
        let stripped = url
            .strip_prefix("socks5://")
            .or_else(|| url.strip_prefix("socks5h://"))
            .unwrap_or(url);

        let (auth, host_port) = if let Some(at_pos) = stripped.rfind('@') {
            let auth_part = &stripped[..at_pos];
            let hp = &stripped[at_pos + 1..];
            let (user, pass) = if let Some(colon) = auth_part.find(':') {
                (
                    auth_part[..colon].to_string(),
                    auth_part[colon + 1..].to_string(),
                )
            } else {
                (auth_part.to_string(), String::new())
            };
            (Some((user, pass)), hp)
        } else {
            (None, stripped)
        };

        // Split host and port
        let (host, port) = if let Some(colon_pos) = host_port.rfind(':') {
            let h = &host_port[..colon_pos];
            let p = host_port[colon_pos + 1..]
                .parse::<u16>()
                .map_err(|_| format!("Invalid proxy port in '{}'", url))?;
            (h.to_string(), p)
        } else {
            return Err(format!(
                "Missing port in proxy address '{}'. Expected format: socks5://host:port",
                url
            ));
        };

        if host.is_empty() {
            return Err("Proxy host cannot be empty".to_string());
        }

        Ok(Self {
            host,
            port,
            username: auth.as_ref().map(|(u, _)| u.clone()),
            password: auth.map(|(_, p)| p),
        })
    }

    /// Resolves the proxy address to a SocketAddr (DNS lookup if needed)
    fn resolve(&self) -> io::Result<SocketAddr> {
        let addr_str = format!("{}:{}", self.host, self.port);
        addr_str
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::AddrNotAvailable, "Cannot resolve proxy address"))
    }

    /// Connects to the target through the SOCKS5 proxy.
    /// The SOCKS5 protocol works in several steps:
    /// 1. Connect to the proxy server
    /// 2. Negotiate the authentication method
    /// 3. Ask the proxy to connect to the target
    /// 4. The proxy confirms, and the TCP stream is then tunneled through
    pub fn connect(
        &self,
        target_host: &str,
        target_port: u16,
        timeout: Duration,
    ) -> io::Result<TcpStream> {
        // Step 1: Open a TCP connection to the proxy server
        let proxy_addr = self.resolve()?;
        let mut stream = TcpStream::connect_timeout(&proxy_addr, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;

        // Step 2: SOCKS5 handshake — tell the proxy which auth methods we support
        let has_auth = self.username.is_some();
        if has_auth {
            // Offer: no auth (0x00) or username/password (0x02)
            stream.write_all(&[0x05, 0x02, 0x00, 0x02])?;
        } else {
            // Offer: no auth only
            stream.write_all(&[0x05, 0x01, 0x00])?;
        }

        // Read the proxy's response to see which method it chose
        let mut response = [0u8; 2];
        stream.read_exact(&mut response)?;

        if response[0] != 0x05 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid SOCKS5 response version",
            ));
        }

        match response[1] {
            0x00 => {
                // No authentication required, proceed
            }
            0x02 => {
                // The proxy requires username/password (RFC 1929)
                self.authenticate(&mut stream)?;
            }
            0xFF => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "SOCKS5 proxy: no acceptable authentication method",
                ));
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("SOCKS5 proxy: unsupported auth method 0x{:02x}", other),
                ));
            }
        }

        // Step 3: Ask the proxy to connect to our target
        let mut request = Vec::with_capacity(64);
        request.push(0x05); // Protocol version
        request.push(0x01); // CONNECT command
        request.push(0x00); // Reserved (always 0)

        // Detect whether the target is an IPv4, IPv6, or domain name
        if let Ok(ipv4) = target_host.parse::<std::net::Ipv4Addr>() {
            request.push(0x01); // Address type: IPv4
            request.extend_from_slice(&ipv4.octets());
        } else if let Ok(ipv6) = target_host.parse::<std::net::Ipv6Addr>() {
            request.push(0x04); // Address type: IPv6
            request.extend_from_slice(&ipv6.octets());
        } else {
            // It's a domain name, send it as-is to the proxy
            let domain = target_host.as_bytes();
            if domain.len() > 255 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Domain name too long for SOCKS5",
                ));
            }
            request.push(0x03); // Address type: domain name
            request.push(domain.len() as u8);
            request.extend_from_slice(domain);
        }

        // Port is sent in big-endian (most significant byte first)
        request.push((target_port >> 8) as u8);
        request.push((target_port & 0xFF) as u8);

        stream.write_all(&request)?;

        // Step 4: Read the proxy's response to check if the connection succeeded
        let mut resp_header = [0u8; 4];
        stream.read_exact(&mut resp_header)?;

        if resp_header[0] != 0x05 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid SOCKS5 connect response version",
            ));
        }

        if resp_header[1] != 0x00 {
            let msg = match resp_header[1] {
                0x01 => "general SOCKS server failure",
                0x02 => "connection not allowed by ruleset",
                0x03 => "network unreachable",
                0x04 => "host unreachable",
                0x05 => "connection refused",
                0x06 => "TTL expired",
                0x07 => "command not supported",
                0x08 => "address type not supported",
                _ => "unknown error",
            };
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                format!("SOCKS5 proxy error: {}", msg),
            ));
        }

        // The proxy sends back the address it bound to — we read it
        // to drain the buffer, but we don't actually need it
        match resp_header[3] {
            0x01 => {
                // IPv4: 4 bytes address + 2 bytes port
                let mut buf = [0u8; 6];
                stream.read_exact(&mut buf)?;
            }
            0x03 => {
                // Domain: 1 byte length + domain + 2 bytes port
                let mut len_buf = [0u8; 1];
                stream.read_exact(&mut len_buf)?;
                let mut buf = vec![0u8; len_buf[0] as usize + 2];
                stream.read_exact(&mut buf)?;
            }
            0x04 => {
                // IPv6: 16 bytes address + 2 bytes port
                let mut buf = [0u8; 18];
                stream.read_exact(&mut buf)?;
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "SOCKS5: unknown address type in response",
                ));
            }
        }

        // All good! The connection is established and the TCP stream now flows
        // through the proxy to the target. Clear the timeouts.
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(None)?;

        Ok(stream)
    }

    /// Sends credentials (username/password) to the SOCKS5 proxy per RFC 1929.
    /// Only called when the proxy requires authentication.
    fn authenticate(&self, stream: &mut TcpStream) -> io::Result<()> {
        let username = self.username.as_deref().unwrap_or("");
        let password = self.password.as_deref().unwrap_or("");

        if username.len() > 255 || password.len() > 255 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "SOCKS5 username or password too long (max 255 bytes)",
            ));
        }

        let mut auth_req = Vec::with_capacity(3 + username.len() + password.len());
        auth_req.push(0x01); // Auth sub-protocol version
        auth_req.push(username.len() as u8);
        auth_req.extend_from_slice(username.as_bytes());
        auth_req.push(password.len() as u8);
        auth_req.extend_from_slice(password.as_bytes());

        stream.write_all(&auth_req)?;

        let mut auth_resp = [0u8; 2];
        stream.read_exact(&mut auth_resp)?;

        if auth_resp[1] != 0x00 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "SOCKS5 authentication failed: invalid credentials",
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let p = Socks5Proxy::parse("socks5://127.0.0.1:1080").unwrap();
        assert_eq!(p.host, "127.0.0.1");
        assert_eq!(p.port, 1080);
        assert!(p.username.is_none());
        assert!(p.password.is_none());
    }

    #[test]
    fn parse_with_auth() {
        let p = Socks5Proxy::parse("socks5://user:pass@proxy.example.com:9050").unwrap();
        assert_eq!(p.host, "proxy.example.com");
        assert_eq!(p.port, 9050);
        assert_eq!(p.username.as_deref(), Some("user"));
        assert_eq!(p.password.as_deref(), Some("pass"));
    }

    #[test]
    fn parse_without_scheme() {
        let p = Socks5Proxy::parse("10.0.0.1:1080").unwrap();
        assert_eq!(p.host, "10.0.0.1");
        assert_eq!(p.port, 1080);
    }

    #[test]
    fn parse_socks5h() {
        let p = Socks5Proxy::parse("socks5h://localhost:1080").unwrap();
        assert_eq!(p.host, "localhost");
        assert_eq!(p.port, 1080);
    }

    #[test]
    fn parse_missing_port() {
        assert!(Socks5Proxy::parse("socks5://127.0.0.1").is_err());
    }
}
