use std::sync::Arc;

use mini_tcp::{device, tcp_packet_loop, ConnectionManager, TcpListener};

pub fn main() -> std::io::Result<()> {
    let mgr = Arc::new(ConnectionManager::new());
    let mgr_ref = Arc::clone(&mgr);

    let mut dev = device::TunDevice::new().unwrap();

    let _ph = std::thread::spawn(move || {
        if let Err(e) = tcp_packet_loop(&mut dev, mgr_ref) {
            eprintln!("error spawning thread: {}", e);
        }
    });

    let addr = "10.0.0.56:3001".parse().unwrap();
    let mut listener = TcpListener::bind(addr, mgr.clone()).unwrap();
    while let Ok((_stream, addr)) = listener.accept() {
        println!("accepted a connection from address: {addr}");
    }
    Ok(())
}
