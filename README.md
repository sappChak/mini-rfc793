## Userspace TCP protocol implementation using TUN device

This project is a minimal, custom implementation of the Transmission Control Protocol
(TCP), as defined in [RFC 793](https://www.rfc-editor.org/rfc/rfc793.html), operating entirely in userspace. It leverages a TUN virtual
network device to process IPv4 and IPv6 packets. The purpose of this project is purely
educational.

## Prerequisites

- Administrative privileges to create and configure TUN devices.
- Tools:
  - `tcpdump`: Packet analyzer.
  - `netcat`: Networking utility for testing.

## Usage

### 1. Compile and run

The `run.sh` script executes the binary with `sudo` to create a TUN device (see `device.rs`), which requires root privileges
due to its interaction with the network stack.

```bash
./scripts/run.sh
```

### 2. Monitor network traffic

Capture and inspect TCP packets on the `tun0` interface using `tcpdump`:

```bash
sudo tcpdump -i tun0 tcp -n
```

### 3. Test with a client

Use `netcat` to establish a TCP connection to the server. The host portions and ports are
up to you:

```bash
nc -N 10.10.0.10 8080
```

or/both(as the program spawns 2 listeners):

```bash
nc -N fd00:dead:beef::10 8081
```

## Acknowledgments

- Portions of the connection management logic are inspired by Jon Gjengset’s TCP implementation.
- Socket API design influenced by Rust's `std::net` module.
