use std::os::fd::{AsFd, BorrowedFd};

use tun_rs::{DeviceBuilder, SyncDevice};

use crate::TUN_MTU;

pub struct TunDevice {
    inner: SyncDevice,
}

impl TunDevice {
    pub fn new() -> crate::Result<TunDevice> {
        let dev = DeviceBuilder::new()
            .ipv4("10.0.0.1", 24, None)
            .ipv6("fd00:dead:beef::1", 64)
            .mtu(TUN_MTU)
            .build_sync()?;

        tracing::info!("TUN device with name '{}' created", dev.name().unwrap());

        dev.set_nonblocking(true)?;

        Ok(TunDevice { inner: dev })
    }

    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.inner.as_fd()
    }

    pub fn send(&self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.send(buf)
    }

    pub fn recv(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.recv(buf)
    }
}
