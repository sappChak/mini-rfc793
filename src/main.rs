use std::{net::SocketAddr, sync::Arc};

use mini_tcp::{
    connections::ConnectionManager,
    device,
    packet_loop::packet_loop,
    tcp::{TcpListener, TcpStream},
};

fn handle_stream(mut stream: TcpStream, addr: SocketAddr) -> std::io::Result<()> {
    let mut buffer = [0; 512];
    let _ = stream.write(b"Hello from server!!!");
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => {
                println!("client {addr} disconnected!");
                break;
            }
            Ok(n) => {
                let received = String::from_utf8_lossy(&buffer[..n]);
                println!("message received: {received:?}");
                if let Err(e) = stream.write(b"extremely important response payload") {
                    eprintln!("failed to send response, {e:?}");
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
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    tracing_subscriber::registry()
        .with(fmt::layer().with_thread_ids(true))
        .with(EnvFilter::from_default_env())
        .init();

    let mut dev = device::TunDevice::new().unwrap();
    let mgr = Arc::new(ConnectionManager::new());

    let mgr_ref = Arc::clone(&mgr);
    std::thread::spawn(move || {
        if let Err(e) = packet_loop(&mut dev, mgr_ref) {
            eprintln!("error spawning thread: {e:?}");
        }
    });

    // launch IPv4 listener on port 8080
    let addr_1 = "10.0.0.9:8080".parse().unwrap();
    let listener_1 = TcpListener::bind(addr_1, mgr.clone()).unwrap();
    std::thread::spawn(move || {
        while let Ok((stream, addr)) = listener_1.accept() {
            println!("accepted a connection: {addr}");
            std::thread::spawn(move || handle_stream(stream, addr));
        }
    });

    // launch IPv6 listener on port 8081
    let addr_2 = "[fd00:dead:beef::5]:8081".parse().unwrap();
    let listener_2 = TcpListener::bind(addr_2, mgr.clone()).unwrap();
    while let Ok((stream, addr)) = listener_2.accept() {
        println!("accepted a connection: {addr}");
        std::thread::spawn(move || handle_stream(stream, addr));
    }

    Ok(())
}
