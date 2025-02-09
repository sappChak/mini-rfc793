use std::{
    collections::VecDeque,
    fmt::Display,
    io::{Error, ErrorKind, Write},
    net::SocketAddrV4,
};

use crate::device::TunDevice;

#[derive(Hash, Eq, PartialEq, Debug)]
pub enum State {
    Listen,
    SynSent,
    SynRcvd,
    Estab,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
    Closed,
}

impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            State::Listen => write!(f, "Listen"),
            State::SynSent => write!(f, "SynSent"),
            State::SynRcvd => write!(f, "SynRcvd"),
            State::Estab => write!(f, "Estab"),
            State::FinWait1 => write!(f, "FinWait1"),
            State::FinWait2 => write!(f, "FinWait2"),
            State::CloseWait => write!(f, "CloseWait"),
            State::Closing => write!(f, "Closing"),
            State::LastAck => write!(f, "LastAck"),
            State::TimeWait => write!(f, "TimeWait"),
            State::Closed => write!(f, "Closed"),
        }
    }
}

// TTL for IPv4
const HOP_LIMIT: u8 = 64;

#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy)]
pub struct SocketPair {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
}

#[derive(Hash, Eq, PartialEq, Debug)]
pub enum ConnectionType {
    Active,
    Passive,
}

// Transmission Control Block
#[derive(Hash, Eq, PartialEq, Debug)]
pub struct Tcb {
    state: State,
    pair: SocketPair,
    connection_type: ConnectionType,
    /// Transmit buffer
    tx_buffer: VecDeque<u8>,
    /// Receive buffer
    rx_buffer: VecDeque<u8>,
    /// Initial seq number of sender
    iss: u32,
    /// Last unacknowledged byte sent
    snd_una: u32,
    /// Next seq number to be sent
    snd_nxt: u32,
    /// Available buffer space for sending
    snd_wnd: u16,
    /// Used for urgent data
    snd_up: u32,
    /// Last segment’s sequence number for window update
    snd_wl1: u32,
    /// Last segment’s acknowledgment number for window update
    snd_wl2: u32,
    /// Initial seq number of receiver
    irs: u32,
    /// Next byte expected from sender
    rcv_nxt: u32,
    /// Available buffer space for receiving
    rcv_wnd: u32,
    /// Used for urgent data
    rcv_up: u32,
}

impl Tcb {
    pub fn new(pair: SocketPair) -> Self {
        Self {
            pair,
            state: State::Listen,
            connection_type: ConnectionType::Passive,
            tx_buffer: VecDeque::with_capacity(4096),
            rx_buffer: VecDeque::with_capacity(4096),
            iss: rand::random::<u32>(),
            snd_una: 0,
            snd_nxt: 0,
            snd_wnd: 0,
            snd_wl1: 0,
            snd_wl2: 0,
            snd_up: 0,
            irs: 0,
            rcv_nxt: 0,
            rcv_wnd: 0,
            rcv_up: 0,
        }
    }

    pub fn poll(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
        payload: &[u8],
    ) -> std::io::Result<()> {
        match self.state {
            State::Listen => self.listen(dev, seg),
            State::SynRcvd => self.syn_rcvd(dev, seg),
            State::Estab => self.estab(dev, seg, payload),
            State::Closed => self.close(dev, seg),
            State::SynSent => self.syn_sent(dev, seg),
            State::CloseWait => Ok(()),
            _ => Ok(()),
        }
    }

    pub fn listen(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
    ) -> std::io::Result<()> {
        // RST should be ignored in LISTEN state
        if seg.rst() {
            return Ok(());
        }
        if seg.ack() {
            self.send_rst(dev, seg.acknowledgment_number())?
        }

        // Security and precedence checks are skipped
        if seg.syn() {
            self.irs = seg.sequence_number();
            self.rcv_nxt = seg.sequence_number().wrapping_add(1);

            let ws = self.window().unwrap();
            let mut tcph = etherparse::TcpHeader::new(
                self.pair.local.port(),
                self.pair.remote.port(),
                self.iss,
                ws,
            );
            tcph.acknowledgment_number = self.rcv_nxt;
            tcph.syn = true;
            tcph.ack = true;

            self.snd_una = self.iss;
            self.snd_nxt = self.iss.wrapping_add(1);

            self.state = State::SynRcvd;

            self.send_datagram(dev, tcph, &[])?
        }
        // It's not ok, actually
        Ok(())
    }

    pub fn syn_rcvd(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
    ) -> std::io::Result<()> {
        if seg.rst() {
            if self.connection_type == ConnectionType::Passive {
                self.state = State::Listen;
                return Ok(());
            } else {
                // TODO: The retrasmission queue should be flushed
                let _ = self.tx_buffer.flush();
                return Err(Error::from(ErrorKind::ConnectionReset));
            }
        }

        if seg.ack() {
            let seg_ack = seg.acknowledgment_number();
            match seg_ack > self.snd_una && seg_ack <= self.snd_nxt {
                true => {
                    if seg.rst() {
                        return Err(Error::from(ErrorKind::ConnectionReset));
                    }
                    self.state = State::Estab;
                    println!("accepted a connection from: {}", self.pair.remote);

                    // TODO: calculate window size precisely
                    self.snd_wnd = seg.window_size();
                    self.snd_wl1 = seg.sequence_number();
                    self.snd_wl2 = seg.acknowledgment_number();
                }
                false => {
                    self.send_rst(dev, seg.sequence_number())?;
                }
            }
        }
        Ok(())
    }

    pub fn syn_sent(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
    ) -> std::io::Result<()> {
        let seg_ack = seg.acknowledgment_number();
        if seg_ack <= self.iss || seg_ack > self.snd_nxt {
            if seg.rst() {
                return Ok(());
            }
            self.send_rst(dev, seg_ack)?
        }

        match seg_ack >= self.snd_una && seg_ack <= self.snd_nxt {
            true => {
                if seg.rst() {
                    return Err(Error::from(ErrorKind::ConnectionReset));
                }
            }
            false => return Ok(()),
        }

        if seg.syn() {
            self.rcv_nxt = seg.sequence_number() + 1;
            self.irs = seg.sequence_number();
            if seg.ack() {
                self.snd_una = seg_ack;
            }

            if self.snd_una > self.iss {
                self.state = State::Estab;

                let ws = seg.window_size();
                let mut tcph = etherparse::TcpHeader::new(
                    self.pair.local.port(),
                    self.pair.remote.port(),
                    self.snd_nxt,
                    ws,
                );
                tcph.acknowledgment_number = self.rcv_nxt;
                tcph.ack = true;

                self.send_datagram(dev, tcph, &[])?
            }
        }
        Ok(())
    }

    pub fn send_rst(&mut self, dev: &mut TunDevice, seq: u32) -> std::io::Result<()> {
        let mut tcph =
            etherparse::TcpHeader::new(self.pair.local.port(), self.pair.remote.port(), seq, 0);
        tcph.rst = true;

        self.send_datagram(dev, tcph, &[])
    }

    pub fn send_rst_ack(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
    ) -> std::io::Result<()> {
        // <SEQ=0><ACK=SEG.SEQ+SEG.LEN><CTL=RST,ACK>
        let seg_len: u32 = seg.slice().len().try_into().unwrap();

        let mut tcph =
            etherparse::TcpHeader::new(self.pair.local.port(), self.pair.remote.port(), 0, 0);

        tcph.acknowledgment_number = seg.sequence_number() + seg_len;
        tcph.rst = true;
        tcph.ack = true;

        self.send_datagram(dev, tcph, &[])
    }

    pub fn estab(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
        payload: &[u8],
    ) -> std::io::Result<()> {
        // 1) check sequence number
        // todo!()

        // 2) check the RST bit,
        if seg.rst() {
            return Err(Error::from(ErrorKind::ConnectionReset));
        }

        // 3) check security and precedence
        // todo!()

        // 4) check the SYN bit,
        // todo!()

        // 5) check the ACK field
        if seg.ack() {
            let seg_ack = seg.acknowledgment_number();
            if self.snd_una < seg_ack && seg_ack <= self.snd_nxt {
                println!("removing acknowledged octets from tx...");
                self.snd_una = seg_ack;
                // TODO: remove acknowledged seqs from tx_buffer. We don't need to store them
                // anymore.
            }
            // todo!()
        }

        if seg.psh() && !payload.is_empty() {
            self.rx_buffer.extend(payload);
            println!("Received a message: {}", String::from_utf8_lossy(payload));
            self.rcv_nxt = self.rcv_nxt.wrapping_add(payload.len() as u32);
            self.send_ack(dev)?
        }

        if seg.fin() {
            // Remote socket won't send anymore!
            println!("connection closing");

            self.state = State::CloseWait;

            self.rcv_nxt = self.rcv_nxt.wrapping_add(1);

            self.send_ack(dev)?

            // TODO: FIN implies PUSH for any segment text not yet delivered to the user.
        }

        Ok(())
    }

    pub fn close(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
    ) -> std::io::Result<()> {
        if !seg.rst() {
            match seg.ack() {
                true => self.send_rst(dev, seg.sequence_number())?,
                false => self.send_rst_ack(dev, seg)?,
            }
        }
        Ok(())
    }

    pub fn close_wait(&mut self, dev: &mut TunDevice) -> std::io::Result<()> {
        Ok(())
    }

    pub fn send_ack(&mut self, dev: &mut TunDevice) -> std::io::Result<()> {
        let ws = self.window().unwrap();
        let mut tcph = etherparse::TcpHeader::new(
            self.pair.local.port(),
            self.pair.remote.port(),
            self.snd_nxt,
            ws,
        );
        tcph.acknowledgment_number = self.rcv_nxt;
        tcph.ack = true;

        self.send_datagram(dev, tcph, &[])
    }

    // Update window size based on the rx buffer
    pub fn window(&self) -> Option<u16> {
        let ws = self.rx_buffer.capacity() - self.rx_buffer.len();
        ws.try_into().ok()
    }

    pub fn send_datagram(
        &self,
        dev: &mut TunDevice,
        tcph: etherparse::TcpHeader,
        payload: &[u8],
    ) -> std::io::Result<()> {
        let builder = etherparse::PacketBuilder::ipv4(
            self.pair.local.ip().octets(),
            self.pair.remote.ip().octets(),
            HOP_LIMIT,
        )
        .tcp_header(tcph);

        let mut datagram = Vec::<u8>::with_capacity(builder.size(payload.len()));

        match builder.write(&mut datagram, payload) {
            Ok(_) => dev.write_all(datagram.as_slice()),
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Packet serialization failed",
            )),
        }
    }
}
