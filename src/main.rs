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

fn main() -> io::Result<()> {
    main_loop()
}
