use std::io::{Read, Write};

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
        Ok(Self { inner })
    }
}

impl Read for TunDevice {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for TunDevice {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
