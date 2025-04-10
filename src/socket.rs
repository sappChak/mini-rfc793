use std::{collections::hash_map::Entry, io, net::SocketAddr, sync::Arc};

use crate::{
    connections::{ConnectionManager, Tuple, TupleV4, TupleV6},
    tcb::Tcb,
};

pub struct Socket {
    mgr: Arc<ConnectionManager>,
    tuple: Tuple,
}

impl Socket {
    pub fn new(addr: SocketAddr, mgr: Arc<ConnectionManager>) -> Socket {
        let tuple = match addr {
            SocketAddr::V4(_) => Tuple::V4(TupleV4::default()),
            SocketAddr::V6(_) => Tuple::V6(TupleV6::default()),
        };
        Socket { mgr, tuple }
    }

    pub fn remote_addr(&self) -> SocketAddr {
        match self.tuple {
            Tuple::V4(tuple_v4) => SocketAddr::V4(tuple_v4.remote),
            Tuple::V6(tuple_v6) => SocketAddr::V6(tuple_v6.remote),
        }
    }

    pub fn local_port(&self) -> u16 {
        match self.tuple {
            Tuple::V4(tuple_v4) => tuple_v4.local.port(),
            Tuple::V6(tuple_v6) => tuple_v6.local.port(),
        }
    }

    pub fn connect(_addr: SocketAddr) -> io::Result<Socket> {
        unimplemented!()
    }

    pub fn bind(&mut self, addr: SocketAddr) -> io::Result<()> {
        let tcb = Tcb::new(addr);
        let mut conns = self.mgr.connections();
        match conns.bound_mut().entry(addr.port()) {
            Entry::Occupied(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "port is already bound",
                ))
            }
            Entry::Vacant(vacant) => {
                match self.tuple {
                    Tuple::V4(ref mut tuple_v4) => {
                        tuple_v4.local = match addr {
                            SocketAddr::V4(socket_addr_v4) => socket_addr_v4,
                            SocketAddr::V6(_) => {
                                panic!("socket was created with AF_INET!");
                            }
                        }
                    }
                    Tuple::V6(ref mut tuple_v6) => {
                        tuple_v6.local = match addr {
                            SocketAddr::V4(_) => {
                                panic!("socket was created with AF_INET6!");
                            }
                            SocketAddr::V6(socket_addr_v6) => socket_addr_v6,
                        }
                    }
                }
                vacant.insert(tcb);
            }
        }
        Ok(())
    }

    pub fn listen(&mut self) {
        let port = self.local_port();
        let mut conns = self.mgr.connections();
        if let Some(tcb) = conns.bound_mut().get_mut(&port) {
            tracing::info!("listening on port {}", port);
            tcb.listen();
        }
    }

    pub fn accept(&self) -> io::Result<Socket> {
        loop {
            let mut conns = self.mgr.connections();
            while conns.pending_mut().is_empty() {
                conns = self.mgr.pending_cvar().wait(conns).unwrap();
            }
            if let Some(tcb) = conns.pending_mut().pop_front() {
                let tuple = match tcb.remote_addr() {
                    Some(remote_addr) => Tuple::new(tcb.listen_addr(), remote_addr),
                    None => panic!("shouldn't have happened!"),
                };
                conns.established_mut().insert(tuple, tcb);

                tracing::info!("accepted a connection from: {}", tuple.remote_port());

                return Ok(Self {
                    mgr: self.mgr.clone(),
                    tuple,
                });
            }
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut conns = self.mgr.connections();
        loop {
            match conns.established_mut().get_mut(&self.tuple) {
                Some(tcb) => {
                    if !tcb.rx_is_empty() {
                        return tcb.read(buf);
                    }
                    if tcb.is_closing() {
                        return Ok(0);
                    }
                    conns = self.mgr.read_cvar().wait(conns).unwrap();
                }
                None => return Ok(0),
            }
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut conns = self.mgr.connections();
        match conns.established_mut().get_mut(&self.tuple) {
            Some(tcb) => tcb.write(buf),
            None => Ok(0),
        }
    }

    pub fn close(&self) {
        let mut conns = self.mgr.connections();
        if let Some(tcb) = conns.established_mut().get_mut(&self.tuple) {
            tcb.init_closing()
        }
    }
}
