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
        match (local, remote) {
            (SocketAddr::V4(local_v4), SocketAddr::V4(remote_v4)) => Tuple::V4(TupleV4 {
                local: local_v4,
                remote: remote_v4,
            }),
            (SocketAddr::V6(local_v6), SocketAddr::V6(remote_v6)) => Tuple::V6(TupleV6 {
                local: local_v6,
                remote: remote_v6,
            }),
            _ => panic!("Invalid address types: {local} and {remote}"),
        }
    }

    pub fn local_ip(&self) -> SocketAddr {
        match self {
            Tuple::V4(tuple_v4) => SocketAddr::V4(tuple_v4.local),
            Tuple::V6(tuple_v6) => SocketAddr::V6(tuple_v6.local),
        }
    }

    pub fn local_port(&self) -> u16 {
        match self {
            Tuple::V4(tuple_v4) => tuple_v4.local.port(),
            Tuple::V6(tuple_v6) => tuple_v6.local.port(),
        }
    }

    pub fn remote_ip(&self) -> SocketAddr {
        match self {
            Tuple::V4(tuple_v4) => SocketAddr::V4(tuple_v4.remote),
            Tuple::V6(tuple_v6) => SocketAddr::V6(tuple_v6.remote),
        }
    }

    pub fn remote_port(&self) -> u16 {
        match self {
            Tuple::V4(tuple_v4) => tuple_v4.remote.port(),
            Tuple::V6(tuple_v6) => tuple_v6.remote.port(),
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
    /// Fully established connections
    established: HashMap<Tuple, Tcb>,
    /// TCBs bound to ports via bind()
    bound: HashMap<u16, Tcb>,
    /// Queue of half-established connections (e.g., SYN received)
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

    pub fn find_in_pending(&mut self, tuple: Tuple) -> Option<&mut Tcb> {
        self.pending
            .iter_mut()
            .find(|tcb| tcb.tuple().unwrap() == tuple)
    }

    pub fn pending_mut(&mut self) -> &mut VecDeque<Tcb> {
        &mut self.pending
    }

    pub fn pending(&self) -> &VecDeque<Tcb> {
        &self.pending
    }

    pub fn established_mut(&mut self) -> &mut HashMap<Tuple, Tcb> {
        &mut self.established
    }

    pub fn established(&self) -> &HashMap<Tuple, Tcb> {
        &self.established
    }

    pub fn bound_mut(&mut self) -> &mut HashMap<u16, Tcb> {
        &mut self.bound
    }

    pub fn bound(&self) -> &HashMap<u16, Tcb> {
        &self.bound
    }
}

#[derive(Default)]
pub struct ConnectionManager {
    /// Mutex to protect the connections data structure
    connections: Mutex<Connections>,
    /// Signals when a pending connection becomes established
    pending_cvar: Condvar,
    /// Signals there's some data to read
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
