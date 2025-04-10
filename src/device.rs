use tun_rs::{DeviceBuilder, SyncDevice};

pub struct TunDevice {
    inner: SyncDevice,
}

impl TunDevice {
    pub fn new() -> crate::Result<TunDevice> {
        let dev = DeviceBuilder::new()
            .ipv4("10.0.0.1", 24, None)
            .ipv6("fd00:dead:beef::1", 64)
            .mtu(1500)
            .build_sync()?;

        dev.set_nonblocking(true)?;

        Ok(TunDevice { inner: dev })
    }

    pub fn send(&self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.send(buf)
    }

    pub fn recv(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.recv(buf)
    }
}
