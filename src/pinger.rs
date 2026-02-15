use colored::Colorize;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::socks5::Socks5Proxy;

pub struct Pinger {
    address: String,
    port: u16,
    timeout: Duration,
    proxy: Option<Socks5Proxy>,
    bind_addr: Option<IpAddr>,
    attempted: u32,
    connected: u32,
    failed: u32,
    times: Vec<f64>,
}

impl Pinger {
    pub fn new(
        address: String,
        port: u16,
        timeout: Duration,
        proxy: Option<Socks5Proxy>,
        bind_addr: Option<IpAddr>,
    ) -> Self {
        Self {
            address,
            port,
            timeout,
            proxy,
            bind_addr,
            attempted: 0,
            connected: 0,
            failed: 0,
            times: Vec::new(),
        }
    }

    pub fn print_header(&self) {
        println!();
        let bind_info = match self.bind_addr {
            Some(ip) => format!(" from  {}", ip.to_string().yellow()),
            None => String::new(),
        };
        if let Some(ref proxy) = self.proxy {
            println!(
                "Connecting to  {}  on TCP  {}{}  via proxy  {}:{}:",
                self.address.green(),
                self.port.to_string().green(),
                bind_info,
                proxy.host.cyan(),
                proxy.port.to_string().cyan()
            );
        } else {
            println!(
                "Connecting to  {}  on TCP  {}{}:",
                self.address.green(),
                self.port.to_string().green(),
                bind_info
            );
        }
        println!();
    }

    fn resolve(&self) -> Option<SocketAddr> {
        let target = format!("{}:{}", self.address, self.port);
        match target.to_socket_addrs() {
            Ok(mut addrs) => addrs.next(),
            Err(_) => None,
        }
    }

    /// Opens a TCP connection to the target address.
    /// If a local interface is specified (via the -i flag), the socket is bound
    /// to that IP before connecting, which forces traffic through the desired
    /// network interface (e.g. VPN, Ethernet, WiFi...).
    fn connect_with_bind(&self, addr: &SocketAddr) -> std::io::Result<TcpStream> {
        match self.bind_addr {
            Some(local_ip) => {
                let domain = if addr.is_ipv4() {
                    Domain::IPV4
                } else {
                    Domain::IPV6
                };
                let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
                let local_addr: SocketAddr = SocketAddr::new(local_ip, 0);
                socket.bind(&local_addr.into())?;
                socket.connect_timeout(&(*addr).into(), self.timeout)?;
                Ok(TcpStream::from(socket))
            }
            None => TcpStream::connect_timeout(addr, self.timeout),
        }
    }

    fn ping(&mut self) {
        self.attempted += 1;

        let start = Instant::now();
        let result = if let Some(ref proxy) = self.proxy {
            // Route through the SOCKS5 proxy to reach the target
            proxy.connect(&self.address, self.port, self.timeout)
        } else {
            // Direct connection, no proxy
            let addr = match self.resolve() {
                Some(a) => a,
                None => {
                    self.failed += 1;
                    println!(
                        "Connection to {} {}: {}",
                        self.address.green(),
                        "failed".red(),
                        "could not resolve address"
                    );
                    return;
                }
            };
            self.connect_with_bind(&addr)
        };

        match result {
            Ok(conn) => {
                let elapsed = start.elapsed();
                drop(conn);
                self.connected += 1;

                let ms = elapsed.as_secs_f64() * 1000.0;
                self.times.push(ms);

                let via = if self.proxy.is_some() {
                    format!("  proxy={}", "SOCKS5".cyan())
                } else {
                    String::new()
                };

                println!(
                    "Connected to {}: time={}  protocol={}  port={}{}",
                    self.address.green(),
                    format!("{:.2}ms", ms).green(),
                    "TCP".green(),
                    self.port.to_string().green(),
                    via
                );
            }
            Err(e) => {
                self.failed += 1;
                println!(
                    "Connection to {} {}: {}",
                    self.address.green(),
                    "failed".red(),
                    e
                );
            }
        }
    }

    /// Pause between each ping. Split into small 100ms chunks so we can
    /// react quickly when the user presses Ctrl+C.
    fn sleep_interruptible(duration: Duration, stop: &Arc<AtomicBool>) {
        let start = Instant::now();
        while start.elapsed() < duration {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn run(&mut self, count: u32, stop: &Arc<AtomicBool>) {
        if count > 0 {
            for i in 0..count {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                self.ping();
                if i < count - 1 && !stop.load(Ordering::SeqCst) {
                    Self::sleep_interruptible(Duration::from_secs(1), stop);
                }
            }
        } else {
            loop {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                self.ping();
                if !stop.load(Ordering::SeqCst) {
                    Self::sleep_interruptible(Duration::from_secs(1), stop);
                }
            }
        }
        println!();
    }

    pub fn print_stats(&self) {
        let fail_pct = if self.attempted > 0 {
            (self.failed as f64 / self.attempted as f64) * 100.0
        } else {
            0.0
        };

        println!("Connection statistics:");
        println!(
            "\tAttempted = {}, Connected = {}, Failed = {}",
            self.attempted.to_string().green(),
            self.connected.to_string().green(),
            format!("{} ({:.1}%)", self.failed, fail_pct).green()
        );

        if !self.times.is_empty() {
            let min = self
                .times
                .iter()
                .cloned()
                .fold(f64::INFINITY, f64::min);
            let max = self
                .times
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max);
            let avg: f64 = self.times.iter().sum::<f64>() / self.times.len() as f64;

            println!("Approximate connection times:");
            println!(
                "\tMinimum = {}, Maximum = {}, Average = {}",
                format!("{:.2}ms", min).green(),
                format!("{:.2}ms", max).green(),
                format!("{:.2}ms", avg).green()
            );
        }
    }
}
