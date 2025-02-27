use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
};

pub struct TunDevice {
    inner: tun::Device,
}

impl TunDevice {
    pub fn new() -> Result<Self, tun::BoxError> {
        let mut config = tun::Configuration::default();

        config
            .address((10, 0, 0, 9))
            .netmask((255, 255, 255, 0))
            .up();

        #[cfg(target_os = "linux")]
        config.platform_config(|config| {
            config.ensure_root_privileges(true);
        });

        let inner = tun::create(&config)?;

        inner.set_nonblock()?;

        Ok(Self { inner })
    }
}

impl AsRawFd for TunDevice {
    fn as_raw_fd(&self) -> std::os::unix::prelude::RawFd {
        self.inner.as_raw_fd()
    }
}

impl Read for TunDevice {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for TunDevice {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
