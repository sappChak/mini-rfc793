pub mod device;

pub mod packet_loop;

pub mod connections;

pub mod socket;

pub mod tcb;

pub mod tcp;

pub mod timers;

const TUN_MTU: u16 = 1500;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub type Result<T> = std::result::Result<T, Error>;
