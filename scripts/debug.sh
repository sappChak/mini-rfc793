#!/bin/sh
cargo build
sudo -E RUST_LOG=debug ./target/debug/mini-tcp
