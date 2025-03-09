use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    io::{self, Read},
    net::SocketAddrV4,
    sync::{Arc, Condvar, Mutex},
};

use tcb::{ConnectionPair, Tcb};
pub mod device;
pub mod tcb;

pub struct Connections {
    /// Established connections
    pub established: HashMap<ConnectionPair, Tcb>,
    /// Bound TCB's in listen state
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
        let mut conns = conn_mgr.connections.lock().unwrap();
        match conns.bound.entry(addr.port()) {
            Entry::Occupied(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "port is already bound",
                ))
            }
            Entry::Vacant(vacant) => {
                // TODO: move the listen into a separate method
                tcb.listen();
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
            while let Some(estab_tcb) = conns.pending.pop_front() {
                let cp = ConnectionPair {
                    local: estab_tcb.listen_addr(),
                    remote: estab_tcb.remote_addr().unwrap(),
                };
                conns.established.insert(cp, estab_tcb);
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
                    let n = tcb.read(buf)?;
                    if n > 0 {
                        return Ok(n);
                    }
                    conns = self.manager.read_cvar.wait(conns).unwrap();
                }
                None => return Ok(0),
            }
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut conns = self.manager.connections.lock().unwrap();
        if let Some(tcb) = conns.established.get_mut(&self.cp) {
            let n = tcb.write(buf)?;
            return Ok(n);
        }
        Ok(5)
    }
}

pub fn packet_loop(dev: &mut device::TunDevice, manager: Arc<ConnectionManager>) -> io::Result<()> {
    let mut buf = [0u8; 1500]; // MTU
    loop {
        {
            let mut conns = manager.connections.lock().unwrap();
            for tcb in conns.established.values_mut() {
                tcb.on_tick(dev)?;
                if !tcb.rx_buffer.is_empty() {
                    manager.read_cvar.notify_one();
                }
            }
        }
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

                            /* Uniquely represents a connection */
                            let cp = ConnectionPair {
                                local: SocketAddrV4::new(dest, tcph.destination_port()),
                                remote: SocketAddrV4::new(src, tcph.source_port()),
                            };

                            let mut conns = manager.connections.lock().unwrap();
                            match conns.established.entry(cp) {
                                Entry::Vacant(_) => {
                                    // Check the pending queue in its own scope:
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
                                                client_tcb.on_segment(dev, &tcph, payload)
                                            })
                                            .is_some()
                                    };

                                    if found_in_pending {
                                        manager.pending_cvar.notify_one();
                                        continue;
                                    }

                                    if let Some(listening_tcb) =
                                        conns.bound.get_mut(&cp.local.port())
                                    {
                                        if let Some(client_tcb) =
                                            listening_tcb.try_accept(dev, &tcph, cp)?
                                        {
                                            conns.pending.push_back(client_tcb);
                                        }
                                    }
                                }
                                Entry::Occupied(mut occupied) => {
                                    if let Err(error) =
                                        occupied.get_mut().on_segment(dev, &tcph, payload)
                                    {
                                        match error.kind() {
                                            io::ErrorKind::ConnectionRefused
                                            | io::ErrorKind::ConnectionReset => {
                                                println!("removing a connection: {:?}", &cp);
                                                conns.established.remove(&cp);
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
