use clap::{Parser, Subcommand};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

mod pinger;
mod socks5;
mod updater;

#[derive(Parser)]
#[command(
    name = "paping",
    version,
    about = "PAPING - TCP port ping utility",
    long_about = "Cross-platform TCP port testing, emulating the functionality of ping (port ping)"
)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
struct Cli {
    /// Target address to ping (IP or domain name)
    address: Option<String>,

    /// Target TCP port
    #[arg(short, long)]
    port: Option<u16>,

    /// Number of pings to send (0 = infinite, Ctrl+C to stop)
    #[arg(short, long, default_value = "0")]
    count: u32,

    /// Maximum wait time for each connection, in milliseconds
    #[arg(short, long, default_value = "1000")]
    timeout: u64,

    /// SOCKS5 proxy (e.g. socks5://127.0.0.1:1080 or socks5://user:pass@host:port)
    #[arg(long)]
    proxy: Option<String>,

    /// Network interface IP to use (useful with a VPN, e.g. 192.168.1.10)
    #[arg(short, long)]
    interface: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Update paping to the latest available version
    Update,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Update) => {
            updater::run_update();
        }
        None => {
            let address = match cli.address {
                Some(addr) => addr,
                None => {
                    eprintln!("Error: address is required. Usage: paping <address> -p <port>");
                    std::process::exit(1);
                }
            };
            let port = match cli.port {
                Some(p) => p,
                None => {
                    eprintln!("Error: --port (-p) is required. Usage: paping <address> -p <port>");
                    std::process::exit(1);
                }
            };

            let proxy = match cli.proxy {
                Some(ref proxy_url) => match socks5::Socks5Proxy::parse(proxy_url) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        eprintln!("Error: invalid proxy: {}", e);
                        std::process::exit(1);
                    }
                },
                None => None,
            };

            let stop = Arc::new(AtomicBool::new(false));
            let stop_clone = stop.clone();

            ctrlc::set_handler(move || {
                stop_clone.store(true, Ordering::SeqCst);
            })
            .expect("Error setting Ctrl-C handler");

            let bind_addr = match cli.interface {
                Some(ref iface) => match iface.parse::<std::net::IpAddr>() {
                    Ok(ip) => Some(ip),
                    Err(_) => {
                        eprintln!("Error: invalid interface IP '{}'", iface);
                        std::process::exit(1);
                    }
                },
                None => None,
            };

            let mut p = pinger::Pinger::new(
                address,
                port,
                std::time::Duration::from_millis(cli.timeout),
                proxy,
                bind_addr,
            );

            p.print_header();
            p.run(cli.count, &stop);
            p.print_stats();
        }
    }
}
