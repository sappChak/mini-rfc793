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
    pub estab_table: HashMap<ConnectionPair, Tcb>,
    /// Bound TCB's in listen state
    pub bind_table: HashMap<u16, Tcb>,
    /// SYN received
    pub pending: VecDeque<Tcb>,
}

impl Connections {
    pub fn new() -> Self {
        Self {
            estab_table: HashMap::new(),
            bind_table: HashMap::new(),
            pending: VecDeque::new(),
        }
    }
}

pub struct ConnectionManager {
    pub connections: Mutex<Connections>,
    pub pending_cvar: Condvar,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(Connections::new()),
            pending_cvar: Condvar::new(),
        }
    }
}

pub struct TcpListener {
    inner: Arc<ConnectionManager>,
}

impl TcpListener {
    pub fn bind(addr: SocketAddrV4, mgr: Arc<ConnectionManager>) -> io::Result<TcpListener> {
        let mut tcb = Tcb::new(addr);
        let mut cons_lock = mgr.connections.lock().unwrap();
        match cons_lock.bind_table.entry(addr.port()) {
            Entry::Occupied(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "port is already bound",
                ))
            }
            Entry::Vacant(vacant) => {
                // TODO: move the listen into separate method
                tcb.listen();
                vacant.insert(tcb);
            }
        }
        Ok(TcpListener { inner: mgr.clone() })
    }

    pub fn accept(&mut self) -> io::Result<(TcpStream, SocketAddrV4)> {
        loop {
            let mut mgr = self.inner.connections.lock().unwrap();
            while mgr.pending.is_empty() {
                mgr = self.inner.pending_cvar.wait(mgr).unwrap();
            }
            while let Some(client_tcb) = mgr.pending.pop_front() {
                let cp = ConnectionPair {
                    local: client_tcb.listen_addr(),
                    remote: client_tcb.remote_addr().unwrap(),
                };
                mgr.estab_table.insert(cp, client_tcb);
                return Ok((
                    TcpStream {
                        inner: self.inner.clone(),
                        cp,
                    },
                    cp.remote,
                ));
            }
        }
    }
}

pub struct TcpStream {
    inner: Arc<ConnectionManager>,
    cp: ConnectionPair,
}

impl TcpStream {
    pub fn connect(_addr: SocketAddrV4) -> io::Result<TcpStream> {
        unimplemented!()
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        todo!()
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        todo!()
    }
}

pub fn tcp_packet_loop(
    dev: &mut device::TunDevice,
    manager: Arc<ConnectionManager>,
) -> io::Result<()> {
    let mut buf = [0u8; 1500]; // MTU
    loop {
        {
            let mut mgr = manager.connections.lock().unwrap();
            for tcb in mgr.estab_table.values_mut() {
                tcb.on_tick(dev)?;
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

                            let mut connections = manager.connections.lock().unwrap();
                            match connections.estab_table.entry(cp) {
                                Entry::Vacant(_) => {
                                    // Check the pending queue in its own scope:
                                    let found_in_pending = {
                                        let pending = &mut connections.pending;
                                        pending
                                            .iter_mut()
                                            .find(|tcb| {
                                                tcph.ack()
                                                    && tcb.listen_addr() == cp.local
                                                    && tcb.remote_addr().unwrap() == cp.remote
                                            })
                                            .map(|client_tcb| {
                                                client_tcb.on_segment(dev, &tcph, payload)
                                            })
                                            .is_some()
                                    };

                                    if found_in_pending {
                                        manager.pending_cvar.notify_one();
                                        continue;
                                    }

                                    if let Some(listening_tcb) =
                                        connections.bind_table.get_mut(&cp.local.port())
                                    {
                                        if let Some(client_tcb) =
                                            listening_tcb.try_accept(dev, &tcph, cp)?
                                        {
                                            connections.pending.push_back(client_tcb);
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
                                                connections.estab_table.remove(&cp);
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
