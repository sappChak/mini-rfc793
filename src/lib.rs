pub mod device;

pub mod ip;

pub mod connections;

pub mod socket;

pub mod tcb;

pub mod tcp;

/// TUN device MTU
const TUN_MTU: usize = 1500;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub type Result<T> = std::result::Result<T, Error>;
