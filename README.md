
# Paping

Cross-platform TCP port testing, emulating the functionality of ping (port ping).

Built with Rust for performance, reliability, and zero runtime dependencies.

## Installation

### From source

```bash
cargo install --path .
```

### Release build

```bash
cargo build --release
```

The binary will be at `target/release/paping` (or `paping.exe` on Windows).

## Usage

### TCP Ping

```bash
paping <address> -p <port>
```

### Options

| Flag | Description | Default |
|------|-------------|---------|
| `-p, --port <PORT>` | Target TCP port (required) | — |
| `-c, --count <COUNT>` | Number of pings (0 = infinite) | `0` |
| `-t, --timeout <TIMEOUT>` | Connection timeout in ms | `1000` |
| `--proxy <PROXY>` | SOCKS5 proxy URL | — |
| `-i, --interface <IP>` | Source IP to bind to (interface) | — |
| `-V, --version` | Print version | — |

### Examples

```bash
# Ping port 443 on 1.1.1.1 indefinitely (Ctrl+C to stop)
paping 1.1.1.1 -p 443

# Ping 10 times
paping 1.1.1.1 -p 443 -c 10

# Ping with 500ms timeout
paping google.com -p 80 -t 500
```

### Interface binding

Bind to a specific network interface (useful with VPN):

```bash
# Use a specific source IP
paping 1.1.1.1 -p 443 -i 192.168.1.10

# Use VPN interface
paping 8.8.8.8 -p 53 -i 10.8.0.2

# Combined with proxy
paping 1.1.1.1 -p 443 -i 192.168.1.10 --proxy socks5://127.0.0.1:1080
```

### SOCKS5 Proxy

Route TCP pings through a SOCKS5 proxy:

```bash
# Via a local SOCKS5 proxy
paping 1.1.1.1 -p 443 --proxy socks5://127.0.0.1:1080

# With authentication
paping 1.1.1.1 -p 443 --proxy socks5://user:pass@proxy.example.com:1080

# Combined with other options
paping google.com -p 80 -c 5 -t 2000 --proxy socks5://10.0.0.1:9050
```

Supported proxy URL formats:
- `socks5://host:port`
- `socks5://user:pass@host:port`
- `socks5h://host:port` (proxy-side DNS resolution)
- `host:port` (scheme optional)

### Self-update

```bash
paping update
```

### Output example

```
Connecting to  1.1.1.1  on TCP  443:

Connected to 1.1.1.1: time=46.10ms  protocol=TCP  port=443
Connected to 1.1.1.1: time=44.96ms  protocol=TCP  port=443
Connected to 1.1.1.1: time=45.65ms  protocol=TCP  port=443
Connected to 1.1.1.1: time=44.98ms  protocol=TCP  port=443
Connected to 1.1.1.1: time=44.55ms  protocol=TCP  port=443

Connection statistics:
        Attempted = 5, Connected = 5, Failed = 0 (0.0%)
Approximate connection times:
        Minimum = 44.55ms, Maximum = 46.10ms, Average = 45.25ms
```

## Authors

- [@Yutho](https://www.github.com/Yutho-tv)

