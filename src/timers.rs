use std::{
    collections::{BinaryHeap, HashMap},
    time::{Duration, Instant},
};

use crate::tcb::TcpFlags;

#[derive(Debug)]
pub struct RTOEntry {
    expires_at: Instant,
    flags: TcpFlags,
    payload_len: usize,
}

impl RTOEntry {
    pub fn expires_at(&self) -> Instant {
        self.expires_at
    }

    pub fn flags(&self) -> &TcpFlags {
        &self.flags
    }

    pub fn payload_len(&self) -> usize {
        self.payload_len
    }
}

#[derive(PartialEq, Eq, Debug)]
struct HeapEntry {
    expires_at: Instant,
    seq: u32,
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.expires_at.cmp(&self.expires_at)
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Default, Debug)]
pub struct TimerManager {
    heap: BinaryHeap<HeapEntry>,
    timers: HashMap<u32, RTOEntry>,
}

impl TimerManager {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            timers: HashMap::new(),
        }
    }

    pub fn start_rto(&mut self, seq: u32, flags: TcpFlags, rto: Duration, payload_len: usize) {
        let expires_at = Instant::now() + rto;
        self.timers.insert(
            seq,
            RTOEntry {
                expires_at,
                flags,
                payload_len,
            },
        );
        self.heap.push(HeapEntry { expires_at, seq })
    }

    pub fn cancel_rto(&mut self, seq: u32) -> Option<RTOEntry> {
        self.timers.remove(&seq)
    }

    pub fn find_expired(&mut self) -> Option<(u32, RTOEntry)> {
        let now = Instant::now();
        while let Some(top) = self.heap.peek() {
            if top.expires_at <= now {
                let top = self.heap.pop().unwrap();
                if let Some(entry) = self.timers.remove(&top.seq) {
                    return Some((top.seq, entry));
                } else {
                    continue; // was canceled, skip
                }
            } else {
                break;
            }
        }
        None
    }

    pub fn find_rto_by_ack<F: FnMut(u32, RTOEntry)>(&mut self, seg_ack: u32, mut f: F) {
        let keys: Vec<u32> = self.timers.keys().cloned().collect();
        for seq in keys {
            if seq <= seg_ack {
                if let Some(entry) = self.timers.remove(&seq) {
                    f(seq, entry);
                }
            }
        }
    }
}
