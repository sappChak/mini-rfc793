pub mod device;

pub mod ip;

pub mod tcb;
use tcb::{ConnectionPair, Tcb};

use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    io::{self},
    net::SocketAddrV4,
    sync::{Arc, Condvar, Mutex},
};

/// TUN device MTU
const TUN_MTU: usize = 1500;

#[derive(Default)]
pub struct Connections {
    /// Established connections
    established: HashMap<ConnectionPair, Tcb>,
    /// Bound TCB's in LISTEN state
    bound: HashMap<u16, Tcb>,
    /// Queue of half-established connections
    pending: VecDeque<Tcb>,
}

impl Connections {
    pub fn new() -> Self {
        Self {
            established: HashMap::new(),
            bound: HashMap::new(),
            pending: VecDeque::new(),
        }
    }

    fn find_in_pending(&mut self, cp: ConnectionPair) -> Option<&mut Tcb> {
        self.pending
            .iter_mut()
            .find(|tcb| tcb.pair().unwrap() == cp)
    }
}

#[derive(Default)]
pub struct ConnectionManager {
    pub connections: Mutex<Connections>,
    pub pending_cvar: Condvar,
    pub read_cvar: Condvar,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(Connections::new()),
            pending_cvar: Condvar::new(),
            read_cvar: Condvar::new(),
        }
    }
}

pub struct TcpListener {
    manager: Arc<ConnectionManager>,
}

impl TcpListener {
    pub fn bind(addr: SocketAddrV4, conn_mgr: Arc<ConnectionManager>) -> io::Result<TcpListener> {
        let mut tcb = Tcb::new(addr);
        tcb.listen();
        let mut conns = conn_mgr.connections.lock().unwrap();
        match conns.bound.entry(addr.port()) {
            Entry::Occupied(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "port is already bound",
                ))
            }
            Entry::Vacant(vacant) => {
                vacant.insert(tcb);
            }
        }
        Ok(TcpListener {
            manager: conn_mgr.clone(),
        })
    }

    pub fn accept(&mut self) -> io::Result<(TcpStream, SocketAddrV4)> {
        loop {
            let mut conns = self.manager.connections.lock().unwrap();
            while conns.pending.is_empty() {
                conns = self.manager.pending_cvar.wait(conns).unwrap();
            }
            if let Some(tcb) = conns.pending.pop_front() {
                let cp = ConnectionPair {
                    local: tcb.listen_addr(),
                    remote: tcb.remote_addr().unwrap(),
                };
                conns.established.insert(cp, tcb);
                return Ok((
                    TcpStream {
                        manager: self.manager.clone(),
                        cp,
                    },
                    cp.remote,
                ));
            }
        }
    }
}

pub struct TcpStream {
    manager: Arc<ConnectionManager>,
    cp: ConnectionPair,
}

impl TcpStream {
    pub fn connect(_addr: SocketAddrV4) -> io::Result<TcpStream> {
        unimplemented!()
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut conns = self.manager.connections.lock().unwrap();
        loop {
            match conns.established.get_mut(&self.cp) {
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
        let mut conns = self.manager.connections.lock().unwrap();
        match conns.established.get_mut(&self.cp) {
            Some(tcb) => tcb.write(buf),
            None => Ok(0),
        }
    }

    pub fn shutdown(&mut self) {
        let mut conns = self.manager.connections.lock().unwrap();
        if let Some(tcb) = conns.established.get_mut(&self.cp) {
            tcb.init_closing()
        }
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        self.shutdown();
    }
}
