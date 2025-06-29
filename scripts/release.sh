#!/bin/sh
cargo build --release
sudo -E RUST_LOG=info ./target/release/mini-tcp
