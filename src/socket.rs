use std::{collections::hash_map::Entry, io, net::SocketAddrV4, sync::Arc};

use crate::{
    tcb::{ConnectionPair, Tcb},
    ConnectionManager,
};

pub struct Socket {
    manager: Arc<ConnectionManager>,
    pair: ConnectionPair,
}

impl Socket {
    pub fn new(manager: Arc<ConnectionManager>) -> Self {
        Self {
            manager,
            pair: ConnectionPair::default(),
        }
    }

    pub fn remote_addr(&self) -> SocketAddrV4 {
        self.pair.remote
    }

    pub fn connect(addr: SocketAddrV4) -> io::Result<Self> {
        unimplemented!()
    }

    pub fn bind(&mut self, addr: SocketAddrV4) -> io::Result<()> {
        let tcb = Tcb::new(addr);

        let mut conns = self.manager.connections();
        match conns.bound.entry(addr.port()) {
            Entry::Occupied(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "port is already bound",
                ))
            }
            Entry::Vacant(vacant) => {
                self.pair.local = addr;
                vacant.insert(tcb);
            }
        }

        Ok(())
    }

    pub fn listen(&mut self) {
        let mut conns = self.manager.connections();
        if let Some(tcb) = conns.bound.get_mut(&self.pair.local.port()) {
            println!("Listening on port {}", self.pair.local.port());
            tcb.listen();
        }
    }

    pub fn accept(&self) -> io::Result<Self> {
        loop {
            let mut conns = self.manager.connections();
            while conns.pending.is_empty() {
                conns = self.manager.pending_cvar.wait(conns).unwrap();
            }
            if let Some(tcb) = conns.pending.pop_front() {
                let cp = ConnectionPair {
                    local: tcb.listen_addr(),
                    remote: tcb.remote_addr().unwrap(),
                };
                conns.established.insert(cp, tcb);
                return Ok(Self {
                    manager: self.manager.clone(),
                    pair: cp,
                });
            }
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut conns = self.manager.connections();
        loop {
            match conns.established.get_mut(&self.pair) {
                Some(tcb) => {
                    if !tcb.rx_is_empty() {
                        return tcb.read(buf);
                    }
                    if tcb.is_closing() {
                        return Ok(0);
                    }
                    conns = self.manager.read_cvar.wait(conns).unwrap();
                }
                None => return Ok(0),
            }
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut conns = self.manager.connections();
        match conns.established.get_mut(&self.pair) {
            Some(tcb) => tcb.write(buf),
            None => Ok(0),
        }
    }

    pub fn close(&self) {
        let mut conns = self.manager.connections();
        if let Some(tcb) = conns.established.get_mut(&self.pair) {
            tcb.init_closing()
        }
    }
}
