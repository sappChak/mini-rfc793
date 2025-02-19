use std::{
    collections::{hash_map::Entry, HashMap},
    io::{self, Read},
    net::SocketAddrV4,
};

use tcb::{SocketPair, Tcb};

pub mod device;
pub mod tcb;
mod timer;

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

pub fn main_loop() -> io::Result<()> {
    let mut dev = device::TunDevice::new().unwrap();
    let mut table = TCBTable::new();

    let ph = std::thread::spawn(move || {
        let mut buf = [0u8; 1500]; // MTU
        loop {
            // TODO: check expired timers, but the read will block

            let amount = dev.read(&mut buf)?;
            let pkt = &buf[0..amount];

            if let Ok(iph) = etherparse::Ipv4HeaderSlice::from_slice(pkt) {
                let src = iph.source_addr();
                let dest = iph.destination_addr();
                // Reject everything not TCP for now
                if iph.protocol() != etherparse::IpNumber::TCP {
                    continue;
                }

                let tcp_offset: usize = (iph.ihl() << 2).into(); // IP header is 4 words long
                match etherparse::TcpHeaderSlice::from_slice(&pkt[tcp_offset..]) {
                    Ok(tcph) => {
                        let data_offset: usize = (tcph.data_offset() << 2).into();
                        let payload = &pkt[tcp_offset + data_offset..];
                        // Uniquely represents a connection
                        let sp = SocketPair {
                            local: SocketAddrV4::new(dest, tcph.destination_port()),
                            remote: SocketAddrV4::new(src, tcph.source_port()),
                        };

                        match table.connections.entry(sp) {
                            // Connection didn't exist before
                            Entry::Vacant(vacant) => {
                                let tcb = Tcb::new(sp);
                                vacant.insert(tcb).poll(&mut dev, tcph, payload)?
                            }

                            // The state is synchronized anyway
                            Entry::Occupied(mut occupied) => {
                                if let Err(error) = occupied.get_mut().poll(&mut dev, tcph, payload)
                                {
                                    match error.kind() {
                                        io::ErrorKind::ConnectionRefused
                                        | io::ErrorKind::ConnectionReset => {
                                            println!("Removing a connection: {:?}", &sp);
                                            table.connections.remove_entry(&sp);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => println!("Error parsing TCP segment {:?}", e),
                }
            }
        }
        #[allow(unreachable_code)]
        Ok::<(), io::Error>(())
    });

    let _ = ph.join();

    Ok(())
}
