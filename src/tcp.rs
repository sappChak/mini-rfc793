use crate::{socket::Socket, ConnectionManager};

use std::{
    io::{self},
    net::SocketAddrV4,
    sync::Arc,
};

pub struct TcpListener {
    inner: Socket,
}

impl TcpListener {
    pub fn bind(addr: SocketAddrV4, conn_mgr: Arc<ConnectionManager>) -> io::Result<TcpListener> {
        let mut sock = Socket::new(conn_mgr.clone());
        sock.bind(addr)?;
        sock.listen();
        Ok(TcpListener { inner: sock })
    }

    pub fn accept(&self) -> io::Result<(TcpStream, SocketAddrV4)> {
        let sock = self.inner.accept()?;
        let addr = sock.remote_addr();
        Ok((TcpStream { inner: sock }, addr))
    }
}

pub struct TcpStream {
    inner: Socket,
}

impl TcpStream {
    pub fn connect(_addr: SocketAddrV4) -> io::Result<TcpStream> {
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
