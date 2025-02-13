use std::{
    collections::hash_map::Entry,
    io::{self, Read},
    net::{Ipv4Addr, SocketAddrV4},
};

use mini_tcp::{
    device, main_loop,
    tcb::{SocketPair, Tcb},
    TCBTable, TcpListener,
};

// fn test() -> io::Result<()> {
//     let addr = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 56), 3000);
//     let listener = TcpListener::bind(addr)?;
//
//     while let Ok((mut stream, _)) = listener.accept() {
//         let mut buffer = [0; 512];
//         loop {
//             match stream.read(&mut buffer) {
//                 Ok(0) => {
//                     println!("Client disconnected");
//                     break;
//                 }
//                 Ok(n) => {
//                     let received = String::from_utf8_lossy(&buffer[..n]);
//                     println!("Received some bytes {:?}", received);
//
//                     if let Err(e) = stream.write(b"Hello world!") {
//                         eprintln!("Failed to send response, {:?}", e);
//                         break;
//                     };
//                 }
//                 Err(_) => {
//                     break;
//                 }
//             }
//         }
//     }
//     Ok(())
// }

fn main() -> io::Result<()> {
    main_loop()
}
