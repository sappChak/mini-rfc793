use std::collections::HashMap;

use tcp::{SocketPair, Tcb};

pub mod device;
pub mod tcp;

pub struct TCBTable {
    pub connections: HashMap<SocketPair, Tcb>,
}

impl Default for TCBTable {
    fn default() -> Self {
        Self::new()
    }
}

impl TCBTable {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }
}
