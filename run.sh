#!/bin/sh
cargo build --release
sudo ./target/release/mini-tcp
# sudo ip route add 10.0.0.9/24 dev tun0
