use std::{net::SocketAddr, sync::Arc};

use mini_tcp::{
    connections::ConnectionManager,
    device,
    ip::packet_loop,
    tcp::{TcpListener, TcpStream},
};

fn handle_stream(mut stream: TcpStream, addr: SocketAddr) -> std::io::Result<()> {
    let mut buffer = [0; 512];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => {
                println!("Client {} disconnected!", addr);
                break;
            }
            Ok(n) => {
                let received = String::from_utf8_lossy(&buffer[..n]);
                println!("Received some bytes {:?}", received);
                if let Err(e) = stream.write(b"omg, don't touch it, it's working!") {
                    eprintln!("Failed to send response, {:?}", e);
                    break;
                };
            }
            Err(_) => {
                break;
            }
        }
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let mut dev = device::TunDevice::new().unwrap();
    let mgr = Arc::new(ConnectionManager::new());

    let mgr_ref = Arc::clone(&mgr);
    let _ph = std::thread::spawn(move || {
        if let Err(e) = packet_loop(&mut dev, mgr_ref) {
            eprintln!("error spawning thread: {}", e);
        }
    });

    let addr = "10.0.0.56:3001".parse().unwrap();
    let listener = TcpListener::bind(addr, mgr.clone()).unwrap();
    while let Ok((stream, addr)) = listener.accept() {
        println!("accepted a connection from address: {addr}");
        std::thread::spawn(move || handle_stream(stream, addr));
    }

    Ok(())
}
