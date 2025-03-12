use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    io::{self, Read},
    net::SocketAddrV4,
    sync::{Arc, Condvar, Mutex},
};

use tcb::{ConnectionPair, Tcb};
pub mod device;
pub mod tcb;

#[derive(Default)]
pub struct Connections {
    /// Established connections
    pub established: HashMap<ConnectionPair, Tcb>,
    /// Bound TCB's in LISTEN state
    pub bound: HashMap<u16, Tcb>,
    /// Queue of half-established connections
    pub pending: VecDeque<Tcb>,
}

impl Connections {
    pub fn new() -> Self {
        Self {
            established: HashMap::new(),
            bound: HashMap::new(),
            pending: VecDeque::new(),
        }
    }
}

#[derive(Default)]
pub struct ConnectionManager {
    pub connections: Mutex<Connections>,
    pub pending_cvar: Condvar,
    pub read_cvar: Condvar,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(Connections::new()),
            pending_cvar: Condvar::new(),
            read_cvar: Condvar::new(),
        }
    }
}

pub struct TcpListener {
    manager: Arc<ConnectionManager>,
}

impl TcpListener {
    pub fn bind(addr: SocketAddrV4, conn_mgr: Arc<ConnectionManager>) -> io::Result<TcpListener> {
        let mut tcb = Tcb::new(addr);
        tcb.listen();
        let mut conns = conn_mgr.connections.lock().unwrap();
        match conns.bound.entry(addr.port()) {
            Entry::Occupied(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "port is already bound",
                ))
            }
            Entry::Vacant(vacant) => {
                vacant.insert(tcb);
            }
        }
        Ok(TcpListener {
            manager: conn_mgr.clone(),
        })
    }

    pub fn accept(&mut self) -> io::Result<(TcpStream, SocketAddrV4)> {
        loop {
            let mut conns = self.manager.connections.lock().unwrap();
            while conns.pending.is_empty() {
                conns = self.manager.pending_cvar.wait(conns).unwrap();
            }
            if let Some(tcb) = conns.pending.pop_front() {
                let cp = ConnectionPair {
                    local: tcb.listen_addr(),
                    remote: tcb.remote_addr().unwrap(),
                };
                println!("new tcb is inserted");
                conns.established.insert(cp, tcb);
                return Ok((
                    TcpStream {
                        manager: self.manager.clone(),
                        cp,
                    },
                    cp.remote,
                ));
            }
        }
    }
}

pub struct TcpStream {
    manager: Arc<ConnectionManager>,
    cp: ConnectionPair,
}

impl TcpStream {
    pub fn connect(_addr: SocketAddrV4) -> io::Result<TcpStream> {
        unimplemented!()
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut conns = self.manager.connections.lock().unwrap();
        loop {
            match conns.established.get_mut(&self.cp) {
                Some(tcb) => {
                    if tcb.has_data() {
                        return tcb.read(buf);
                    }
                    if tcb.is_closing() {
                        return Ok(0);
                    }
                    conns = self.manager.read_cvar.wait(conns).unwrap(); // releases the lock
                }
                None => return Ok(0), // it's some kind of an error, deal with it later
            }
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut conns = self.manager.connections.lock().unwrap();
        loop {
            match conns.established.get_mut(&self.cp) {
                Some(tcb) => {
                    let n = tcb.write(buf)?;
                    if n > 0 {
                        return Ok(n);
                    }
                    // TODO:
                }
                None => return Ok(0),
            }
        }
    }

    pub fn shutdown(&mut self) {
        let mut conns = self.manager.connections.lock().unwrap();
        if let Some(tcb) = conns.established.get_mut(&self.cp) {
            tcb.close()
        }
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Drop for TcpListener {
    fn drop(&mut self) {
        println!("bye, bye my dear listener");
    }
}

pub fn packet_loop(dev: &mut device::TunDevice, manager: Arc<ConnectionManager>) -> io::Result<()> {
    let mut buf = [0u8; 1500]; // MTU
    loop {
        let mut conns = manager.connections.lock().unwrap();
        for tcb in conns.established.values_mut() {
            tcb.on_tick(dev)?;
        }
        // not so effective, deal with it later
        conns.established.retain(|_, tcb| !tcb.is_closed());
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
                                    let found_in_pending = {
                                        let pending = &mut conns.pending;
                                        pending
                                            .iter_mut()
                                            .find(|tcb| {
                                                tcb.listen_addr() == cp.local
                                                    && tcb.remote_addr().unwrap() == cp.remote
                                            })
                                            .map(|client_tcb| {
                                                // try to complete 3-WH
                                                client_tcb.on_segment(
                                                    dev,
                                                    &tcph,
                                                    payload,
                                                    &manager.read_cvar,
                                                )
                                            })
                                            .is_some()
                                    };
                                    if found_in_pending {
                                        // notify accept() about an established connection
                                        manager.pending_cvar.notify_all();
                                        continue;
                                    }

                                    // connection wasn't initialized
                                    if let Some(listening_tcb) =
                                        conns.bound.get_mut(&cp.local.port())
                                    {
                                        if let Some(client_tcb) =
                                            listening_tcb.try_establish(dev, &tcph, cp)?
                                        {
                                            conns.pending.push_back(client_tcb);
                                        }
                                    }
                                }
                                Entry::Occupied(mut occupied) => {
                                    if let Err(error) = occupied.get_mut().on_segment(
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
