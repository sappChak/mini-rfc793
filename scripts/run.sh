#!/bin/sh
cargo build --release
sudo ./target/release/mini-tcp
