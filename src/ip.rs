use std::{
    collections::hash_map::Entry,
    io::{self, Read},
    net::SocketAddrV4,
    sync::Arc,
};

use crate::{device, ConnectionManager, ConnectionPair, TUN_MTU};

pub fn packet_loop(dev: &mut device::TunDevice, manager: Arc<ConnectionManager>) -> io::Result<()> {
    let mut buf = [0u8; TUN_MTU]; // MTU
    loop {
        let mut conns = manager.connections.lock().unwrap();
        for tcb in conns.established.values_mut() {
            tcb.on_tick(dev)?;
        }
        drop(conns);

        match dev.read(&mut buf) {
            Ok(n) => {
                let pkt = &buf[0..n];
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
                            /* uniquely represents a connection */
                            let cp = ConnectionPair {
                                local: SocketAddrV4::new(dest, tcph.destination_port()),
                                remote: SocketAddrV4::new(src, tcph.source_port()),
                            };

                            let mut conns = manager.connections.lock().unwrap();
                            match conns.established.entry(cp) {
                                Entry::Vacant(_) => {
                                    // it's likely, the connection was already initialized:
                                    if let Some(client) = conns.find_in_pending(cp) {
                                        client.on_segment(
                                            dev,
                                            &tcph,
                                            payload,
                                            &manager.read_cvar,
                                        )?;
                                        manager.pending_cvar.notify_all(); // notify accept() about an established connection
                                        continue;
                                    }
                                    // connection wasn't initialized, try to establish one
                                    if let Some(listener) = conns.bound.get_mut(&cp.local.port()) {
                                        if let Some(client) =
                                            listener.try_establish(dev, &tcph, cp)?
                                        {
                                            conns.pending.push_back(client);
                                        }
                                    }
                                }
                                Entry::Occupied(mut o) => {
                                    if let Err(error) = o.get_mut().on_segment(
                                        dev,
                                        &tcph,
                                        payload,
                                        &manager.read_cvar,
                                    ) {
                                        match error.kind() {
                                            io::ErrorKind::ConnectionRefused
                                            | io::ErrorKind::ConnectionReset => {
                                                println!("removing a connection: {:?}", &cp);
                                                conns.established.remove(&cp);
                                                manager.read_cvar.notify_all();
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => println!("error parsing TCP segment {:?}", e),
                    }
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(e),
        }
    }
}
