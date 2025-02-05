use std::collections::HashMap;

use tcb::{SocketPair, Tcb};

pub mod device;
pub mod tcb;

#[derive(Default)]
pub struct TCBTable {
    pub connections: HashMap<SocketPair, Tcb>,
}

impl TCBTable {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }
}
