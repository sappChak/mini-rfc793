use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    io::{self, Read},
    net::SocketAddrV4,
    sync::{Arc, Condvar, Mutex},
};

use tcb::{ConnectionPair, Socket};
pub mod device;
pub mod tcb;

pub struct Connections {
    pub connections: HashMap<ConnectionPair, Socket>,
    pub pending: VecDeque<ConnectionPair>,
}

impl Connections {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
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
    pub fn bind(addr: SocketAddrV4) -> io::Result<TcpListener> {
        let mut dev = device::TunDevice::new().unwrap();
        let mgr = Arc::new(ConnectionManager::new());

        let mut sock = Socket::new(addr);
        sock.listen();

        let mgr_ref = Arc::clone(&mgr);
        let _h = std::thread::spawn(move || {
            if let Err(e) = packet_loop(&mut dev, mgr_ref) {
                eprintln!("error spawning thread: {}", e);
            }
        });

        Ok(TcpListener { inner: mgr.clone() })
    }

    pub fn accept(&mut self) -> io::Result<(TcpStream, SocketAddrV4)> {
        loop {
            let mut mgr = self.inner.connections.lock().unwrap();
            while mgr.pending.is_empty() {
                mgr = self.inner.pending_cvar.wait(mgr).unwrap();
            }
            println!("received notify from the packet_loop");
            if let Some(cp) = mgr.pending.pop_front() {
                if let Some(sock) = mgr.connections.get_mut(&cp) {
                    let sock = sock.accept()?;
                    let addr = sock.remote().unwrap();
                    return Ok((TcpStream { inner: sock }, addr));
                }
            }
        }
    }
}

pub struct TcpStream {
    inner: Socket,
}

impl TcpStream {
    pub fn connect(_addr: SocketAddrV4) -> io::Result<TcpStream> {
        unimplemented!()
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
}

pub fn packet_loop(dev: &mut device::TunDevice, manager: Arc<ConnectionManager>) -> io::Result<()> {
    let mut buf = [0u8; 1500]; // MTU
    loop {
        {
            let mut mgr = manager.connections.lock().unwrap();
            for tcb in mgr.connections.values_mut() {
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

                            let mut conns_lock = manager.connections.lock().unwrap();
                            match conns_lock.connections.entry(cp) {
                                Entry::Vacant(vacant) => {
                                    conns_lock.pending.push_back(cp);
                                    manager.pending_cvar.notify_one();
                                }
                                Entry::Occupied(mut occupied) => {
                                    if let Err(error) =
                                        occupied.get_mut().on_segment(dev, cp.remote, tcph, payload)
                                    {
                                        match error.kind() {
                                            io::ErrorKind::ConnectionRefused
                                            | io::ErrorKind::ConnectionReset => {
                                                println!("removing a connection: {:?}", &cp);
                                                conns_lock.connections.remove(&cp);
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
