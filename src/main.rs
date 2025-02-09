use std::{
    collections::hash_map::Entry,
    io::{ErrorKind, Read},
    net::SocketAddrV4,
};

use mini_tcp::{
    device::TunDevice,
    tcb::{SocketPair, Tcb},
    TCBTable,
};

fn main() -> std::io::Result<()> {
    let mut dev = TunDevice::new().unwrap();
    let mut tcb_table = TCBTable::new();
    let mut buf = [0u8; 1500]; // MTU

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

            let tcp_offset: usize = (ipd.ihl() << 2).into(); // IP header is 4 words long
            match etherparse::TcpHeaderSlice::from_slice(&pkt[tcp_offset..]) {
                Ok(tcph) => {
                    let data_offset: usize = (tcph.data_offset() << 2).into();
                    let payload = &pkt[tcp_offset + data_offset..];
                    // Uniquely represents a connection
                    let sp = SocketPair {
                        local: SocketAddrV4::new(dest, tcph.destination_port()),
                        remote: SocketAddrV4::new(src, tcph.source_port()),
                    };

                    match tcb_table.connections.entry(sp) {
                        Entry::Vacant(vacant) => {
                            let tcb = Tcb::new(sp);
                            vacant.insert(tcb).poll(&mut dev, tcph, payload)?
                        }
                        Entry::Occupied(mut occupied) => {
                            if let Err(error) = occupied.get_mut().poll(&mut dev, tcph, payload) {
                                match error.kind() {
                                    ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset => {
                                        println!("Removing a connection: {:?}", &sp);
                                        tcb_table.connections.remove_entry(&sp);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Err(e) => println!("Error parsing TCP segment, bruh... {:?}", e),
            }
        }
    }
}
