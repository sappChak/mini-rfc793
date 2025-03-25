use std::{
    collections::{HashMap, VecDeque},
    sync::{Condvar, Mutex},
};

pub mod device;

pub mod ip;

pub mod socket;
pub mod tcb;
use tcb::{ConnectionPair, Tcb};

pub mod tcp;

/// TUN device MTU
const TUN_MTU: usize = 1500;

#[derive(Default)]
pub struct Connections {
    /// Established connections
    established: HashMap<ConnectionPair, Tcb>,
    /// TCB's in LISTEN state
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

    pub fn find_in_pending(&mut self, cp: ConnectionPair) -> Option<&mut Tcb> {
        self.pending
            .iter_mut()
            .find(|tcb| tcb.pair().unwrap() == cp)
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
}
