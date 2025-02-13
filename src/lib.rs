use std::{
    collections::{hash_map::Entry, HashMap},
    io::{self, Read, Write},
    net::SocketAddrV4,
};

use tcb::{SocketPair, Tcb};

pub mod device;
pub mod tcb;

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

pub struct TcpListener {
    inner: Tcb,
}

pub struct TcpStream {
    inner: Tcb,
}

impl TcpListener {
    pub fn bind(addr: SocketAddrV4) -> io::Result<TcpListener> {
        // socket()
        // bind()
        // listen()

        todo!()
    }

    pub fn accept(&self) -> io::Result<(TcpStream, SocketAddrV4)> {
        todo!()
    }
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        todo!()
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        todo!()
    }

    fn flush(&mut self) -> io::Result<()> {
        todo!()
    }
}

pub fn main_loop() -> io::Result<()> {
    let mut dev = device::TunDevice::new().unwrap();
    let mut table = TCBTable::new();

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

                    match table.connections.entry(sp) {
                        Entry::Vacant(vacant) => {
                            let tcb = Tcb::new(sp);
                            match vacant.insert(tcb).poll(tcph, payload) {
                                tcb::PollResult::SendDatagram(segment) => {
                                    dev.write_all(segment.as_slice())?
                                }
                                _ => {}
                            }
                        }
                        Entry::Occupied(mut occupied) => {
                            match occupied.get_mut().poll(tcph, payload) {
                                tcb::PollResult::SendDatagram(segment) => {
                                    dev.write_all(segment.as_slice())?
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(e) => println!("Error parsing TCP segment, bruh... {:?}", e),
            }
        }

        // Poll timers here + on_tick
    }
}
