use crate::{connections::ConnectionManager, socket::Socket};

use std::{
    io::{self},
    net::SocketAddr,
    sync::Arc,
};

pub struct TcpListener {
    inner: Socket,
}

impl TcpListener {
    pub fn bind(addr: SocketAddr, manager: Arc<ConnectionManager>) -> io::Result<TcpListener> {
        let mut sock = Socket::new(addr, manager.clone());
        // Bind socket
        sock.bind(addr)?;

        // Start listening
        sock.listen();
        Ok(TcpListener { inner: sock })
    }

    pub fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        let sock = self.inner.accept()?;
        let addr = sock.remote_addr();
        Ok((TcpStream { inner: sock }, addr))
    }
}

pub struct TcpStream {
    inner: Socket,
}

impl TcpStream {
    pub fn connect(_addr: SocketAddr) -> io::Result<TcpStream> {
        unimplemented!()
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    pub fn shutdown(&mut self) {
        self.inner.close();
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        self.shutdown();
    }
}
