#!/bin/sh
cargo build --release
sudo RUST_LOG=info ./target/release/mini-tcp
