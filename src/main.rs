use std::{collections::hash_map::Entry, io::Read, net::SocketAddrV4};

use mini_tcp::{
    device::TunDevice,
    tcb::{ConnectionError, SocketPair, Tcb},
    TCBTable,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = TunDevice::new().unwrap();
    let mut tcb_table = TCBTable::new();
    let mut buf = [0u8; 1500]; // MTU

    // What's the next step of the operation?
    loop {
        let amount = dev.read(&mut buf)?;
        let pkt = &buf[0..amount];

        if let Ok(ipd) = etherparse::Ipv4HeaderSlice::from_slice(pkt) {
            let src = ipd.source_addr();
            let dest = ipd.destination_addr();
            // Reject everything not TCP for now
            if ipd.protocol() != etherparse::IpNumber::TCP {
                continue;
            }

            let iph_end: usize = (ipd.ihl() << 2).into(); // IP header is 4 words long
            match etherparse::TcpHeaderSlice::from_slice(&pkt[iph_end..]) {
                Ok(tcp) => {
                    println!("TCP {} -> {}", tcp.source_port(), tcp.destination_port());
                    // Uniquely represents a connection
                    let sp = SocketPair {
                        local: SocketAddrV4::new(dest, tcp.destination_port()),
                        remote: SocketAddrV4::new(src, tcp.source_port()),
                    };
                    match tcb_table.connections.entry(sp) {
                        Entry::Vacant(vacant) => {
                            let tcb = Tcb::new(sp);
                            vacant.insert(tcb).poll(&mut dev, tcp)?
                        }
                        Entry::Occupied(mut occupied) => {
                            if let Err(
                                ConnectionError::ConnectionReset
                                | ConnectionError::ConnectionRefused,
                            ) = occupied.get_mut().poll(&mut dev, tcp)
                            {
                                tcb_table.connections.remove_entry(&sp);
                            }
                        }
                    }
                }
                Err(e) => println!("Error parsing TCP segment, bruh... {:?}", e),
            }
        }
    }
}
