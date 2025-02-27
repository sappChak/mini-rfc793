use mini_tcp::{device, packet_loop};

fn main() -> std::io::Result<()> {
    let mut dev = device::TunDevice::new().unwrap();

    let ph = std::thread::spawn(move || packet_loop(&mut dev));
    let _ = ph.join();

    Ok(())
}
