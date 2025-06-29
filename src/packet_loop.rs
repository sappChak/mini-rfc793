use std::{
    collections::hash_map::Entry,
    io::{self},
    net::{SocketAddrV4, SocketAddrV6},
    sync::Arc,
};

use crate::{
    TUN_MTU,
    connections::{ConnectionManager, Tuple, TupleV4, TupleV6},
    device,
};

pub fn packet_loop(dev: &mut device::TunDevice, mgr: Arc<ConnectionManager>) -> io::Result<()> {
    let mut buf = [0u8; TUN_MTU as usize];
    loop {
        use nix::poll::{PollFd, PollFlags, PollTimeout};
        let mut pfd = [PollFd::new(dev.as_fd(), PollFlags::POLLIN)];
        let nready = nix::poll::poll(&mut pfd[..], PollTimeout::from(10u16)).unwrap();
        // check timers and tx buffer if there is no incoming packet
        if nready == 0 {
            let mut conns = mgr.connections();
            for tcb in conns.established_mut().values_mut() {
                tcb.on_tick(dev)?;
            }
            continue;
        }
        match dev.recv(&mut buf) {
            Ok(n) => {
                let pkt = &buf[0..n];
                process_packet(dev, mgr.clone(), pkt)?;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(e),
        }
    }
}

fn process_packet(
    dev: &mut device::TunDevice,
    mgr: Arc<ConnectionManager>,
    pkt: &[u8],
) -> io::Result<()> {
    if let Ok(ipv4_hdr) = etherparse::Ipv4HeaderSlice::from_slice(pkt) {
        let src = ipv4_hdr.source_addr();
        let dest = ipv4_hdr.destination_addr();
        // Reject everything not TCP for now
        if ipv4_hdr.protocol() != etherparse::IpNumber::TCP {
            return Ok(());
        }
        let tcp_offset: usize = (ipv4_hdr.ihl() << 2).into(); // IPv4 header is 4 words long
        match etherparse::TcpHeaderSlice::from_slice(&pkt[tcp_offset..]) {
            Ok(tcph) => {
                let data_offset: usize = (tcph.data_offset() << 2).into();
                let payload = &pkt[tcp_offset + data_offset..];
                /* uniquely represents a connection */
                let tuple = Tuple::V4(TupleV4 {
                    local: SocketAddrV4::new(dest, tcph.destination_port()),
                    remote: SocketAddrV4::new(src, tcph.source_port()),
                });
                process_tcp_slice(dev, mgr.clone(), tcph, payload, tuple)?;
            }
            Err(e) => tracing::warn!("error parsing TCP segment {:?}", e),
        }
    } else if let Ok(ipv6_hdr) = etherparse::Ipv6HeaderSlice::from_slice(pkt) {
        let src = ipv6_hdr.source_addr();
        let dest = ipv6_hdr.destination_addr();
        // Reject everything not TCP for now
        if ipv6_hdr.next_header() != etherparse::IpNumber::TCP {
            return Ok(());
        }
        let tcp_offset: usize = ipv6_hdr.slice().len();
        match etherparse::TcpHeaderSlice::from_slice(&pkt[tcp_offset..]) {
            Ok(tcph) => {
                let data_offset: usize = (tcph.data_offset() << 2).into();
                let payload = &pkt[tcp_offset + data_offset..];
                /* uniquely represents a connection */
                let tuple = Tuple::V6(TupleV6 {
                    local: SocketAddrV6::new(dest, tcph.destination_port(), 0, 0),
                    remote: SocketAddrV6::new(src, tcph.source_port(), 0, 0),
                });
                process_tcp_slice(dev, mgr.clone(), tcph, payload, tuple)?;
            }
            Err(e) => tracing::warn!("error parsing TCP segment {:?}", e),
        }
    }

    Ok(())
}

fn process_tcp_slice(
    dev: &mut device::TunDevice,
    mgr: Arc<ConnectionManager>,
    tcph: etherparse::TcpHeaderSlice,
    payload: &[u8],
    tuple: Tuple,
) -> io::Result<()> {
    let mut conns = mgr.connections();

    match conns.established_mut().entry(tuple) {
        Entry::Vacant(_) => {
            // it's likely, the connection was already initialized:
            if let Some(client) = conns.find_in_pending(tuple) {
                client.on_segment(dev, &tcph, payload, mgr.read_cvar())?;
                mgr.pending_cvar().notify_all(); // notify accept() about an established connection
                return Ok(());
            }
            // connection wasn't initialized, try to establish one
            if let Some(listener) = conns.bound_mut().get_mut(&tuple.local_port()) {
                if let Some(client) = listener.try_establish(dev, &tcph, tuple)? {
                    conns.pending_mut().push_back(client);
                }
            }
        }
        Entry::Occupied(mut o) => {
            if let Err(error) = o.get_mut().on_segment(dev, &tcph, payload, mgr.read_cvar()) {
                match error.kind() {
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionReset => {
                        tracing::info!("removing a connection: {:?}", &tuple);
                        conns.established_mut().remove(&tuple);
                        mgr.read_cvar().notify_all();
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
