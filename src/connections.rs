use std::{
    collections::{HashMap, VecDeque},
    net::{SocketAddr, SocketAddrV4, SocketAddrV6},
    sync::{Condvar, Mutex},
};

use crate::tcb::Tcb;

#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy)]
pub enum Tuple {
    V4(TupleV4),
    V6(TupleV6),
}

#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy)]
pub struct TupleV4 {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
}

impl Default for TupleV4 {
    fn default() -> Self {
        Self {
            local: SocketAddrV4::new([0; 4].into(), 0),
            remote: SocketAddrV4::new([0; 4].into(), 0),
        }
    }
}

impl Tuple {
    pub fn new(local: SocketAddr, remote: SocketAddr) -> Tuple {
        match local {
            SocketAddr::V4(local_v4) => Tuple::V4(TupleV4 {
                local: local_v4,
                remote: match remote {
                    SocketAddr::V4(remote_v4) => remote_v4,
                    _ => panic!("remote address is not V4!"),
                },
            }),
            SocketAddr::V6(local_v6) => Tuple::V6(TupleV6 {
                local: local_v6,
                remote: match remote {
                    SocketAddr::V6(remote_v6) => remote_v6,
                    _ => panic!("remote address is not V6!"),
                },
            }),
        }
    }

    pub fn local_ip(&self) -> SocketAddr {
        match self {
            Tuple::V4(cp_v4) => SocketAddr::V4(cp_v4.local),
            Tuple::V6(cp_v6) => SocketAddr::V6(cp_v6.local),
        }
    }

    pub fn local_port(&self) -> u16 {
        match self {
            Tuple::V4(cp_v4) => cp_v4.local.port(),
            Tuple::V6(cp_v6) => cp_v6.local.port(),
        }
    }

    pub fn remote_ip(&self) -> SocketAddr {
        match self {
            Tuple::V4(cp_v4) => SocketAddr::V4(cp_v4.remote),
            Tuple::V6(cp_v6) => SocketAddr::V6(cp_v6.remote),
        }
    }

    pub fn remote_port(&self) -> u16 {
        match self {
            Tuple::V4(cp_v4) => cp_v4.remote.port(),
            Tuple::V6(cp_v6) => cp_v6.remote.port(),
        }
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy)]
pub struct TupleV6 {
    pub local: SocketAddrV6,
    pub remote: SocketAddrV6,
}

impl Default for TupleV6 {
    fn default() -> Self {
        Self {
            local: SocketAddrV6::new([0; 16].into(), 0, 0, 0),
            remote: SocketAddrV6::new([0; 16].into(), 0, 0, 0),
        }
    }
}

#[derive(Eq, PartialEq, Debug)]
pub enum ConnectionType {
    Active,
    Passive,
}

#[derive(Default)]
pub struct Connections {
    /// Established connections
    pub established: HashMap<Tuple, Tcb>,
    /// TCB's created with bind()
    pub bound: HashMap<u16, Tcb>,
    /// Queue of half-established connections
    pub pending: VecDeque<Tcb>,
}

impl Connections {
    pub fn new() -> Self {
        Self {
            established: HashMap::new(),
            bound: HashMap::new(),
            pending: VecDeque::new(),
        }
    }

    pub fn find_in_pending(&mut self, cp: Tuple) -> Option<&mut Tcb> {
        self.pending
            .iter_mut()
            .find(|tcb| tcb.tuple().unwrap() == cp)
    }
}

#[derive(Default)]
pub struct ConnectionManager {
    connections: Mutex<Connections>,
    pending_cvar: Condvar,
    read_cvar: Condvar,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(Connections::new()),
            pending_cvar: Condvar::new(),
            read_cvar: Condvar::new(),
        }
    }

    pub fn connections(&self) -> std::sync::MutexGuard<Connections> {
        self.connections.lock().unwrap()
    }

    pub fn read_cvar(&self) -> &Condvar {
        &self.read_cvar
    }

    pub fn pending_cvar(&self) -> &Condvar {
        &self.pending_cvar
    }
}
