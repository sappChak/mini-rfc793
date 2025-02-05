use std::{collections::VecDeque, fmt::Display, io::Write, net::SocketAddrV4};

use crate::device::TunDevice;

#[derive(thiserror::Error, Debug)]
pub enum ConnectionError {
    #[error("connection reset by peer")]
    ConnectionReset,
    #[error("connection refused")]
    ConnectionRefused,
}

type TcbResult<T> = std::result::Result<T, ConnectionError>;

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

// Transmission Control Block
#[derive(Hash, Eq, PartialEq, Debug)]
pub struct Tcb {
    state: State,
    pair: SocketPair,
    tx_buffer: VecDeque<u8>,
    rx_buffer: VecDeque<u8>,
    snd_una: u32,
    snd_nxt: u32,
    snd_wnd: u16,
    snd_wl1: u32,
    snd_wl2: u32,
    iss: u32,
    rcv_nxt: u32,
    rcv_wnd: u32,
    irs: u32,
}

impl Tcb {
    pub fn new(pair: SocketPair) -> Self {
        Self {
            pair,
            state: State::Listen,
            tx_buffer: VecDeque::with_capacity(4096),
            rx_buffer: VecDeque::with_capacity(4096),
            snd_una: 0,
            snd_nxt: 0,
            snd_wnd: 0,
            snd_wl1: 0,
            snd_wl2: 0,
            iss: rand::random::<u32>(),
            rcv_nxt: 0,
            rcv_wnd: 0,
            irs: 0,
        }
    }

    pub fn poll(&mut self, dev: &mut TunDevice, seg: etherparse::TcpHeaderSlice) -> TcbResult<()> {
        match self.state {
            State::Listen => self.process_listen(dev, seg),
            State::SynRcvd => self.process_syn_rcvd(dev, seg),
            State::Estab => self.process_estab(dev, seg),
            State::Closed => self.process_close(dev, seg),
            State::SynSent => self.process_syn_sent(dev, seg),
            _ => Ok(()),
        }
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

    pub fn process_listen(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
    ) -> TcbResult<()> {
        // RST should be ignored in LISTEN state
        if seg.rst() {
            return Ok(());
        }
        if seg.ack() {
            self.send_rst(dev, seg.acknowledgment_number()).unwrap();
            return Ok(());
        }

        // Security and precedence checks are skipped
        if seg.syn() {
            self.rcv_nxt = seg.sequence_number() + 1;
            self.irs = seg.sequence_number();

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

            self.snd_nxt = self.iss + 1;
            self.snd_una = self.iss;

            self.state = State::SynRcvd;

            self.send_datagram(dev, tcph, &[]).unwrap();
        }
        // It's not ok, actually
        Ok(())
    }

    pub fn process_syn_sent(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
    ) -> TcbResult<()> {
        let seg_ack = seg.acknowledgment_number();
        if seg_ack <= self.iss || seg_ack > self.snd_nxt {
            if seg.rst() {
                return Ok(());
            }
            self.send_rst(dev, seg_ack).unwrap();
        }

        match seg_ack >= self.snd_una && seg_ack <= self.snd_nxt {
            true => {
                if seg.rst() {
                    self.state = State::Closed;
                    return Err(ConnectionError::ConnectionReset);
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

                let _ = self.send_datagram(dev, tcph, &[]);
            }
        }
        Ok(())
    }

    pub fn process_syn_rcvd(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
    ) -> TcbResult<()> {
        if seg.rst() {
            // Only in case of passive open
            self.state = State::Listen;
            return Ok(());
            // Active open may appear here only in case
            // of simultaneous open. The retransmission
            // queue should be flushed.
            // Delete the TCB.
        }

        if seg.ack() {
            let seg_ack = seg.acknowledgment_number();
            match seg_ack > self.snd_una && seg_ack <= self.snd_nxt {
                true => {
                    if seg.rst() {
                        self.state = State::Closed;
                        return Err(ConnectionError::ConnectionReset);
                    }
                    self.state = State::Estab;
                    self.snd_wnd = seg.window_size();
                    self.snd_wl1 = seg.sequence_number();
                    self.snd_wl2 = seg.acknowledgment_number();
                }
                false => {
                    self.send_rst(dev, seg.sequence_number()).unwrap();
                }
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
        // let seg_len: u32 = seg.to_header().header_len().try_into().unwrap();
        let mut tcph =
            etherparse::TcpHeader::new(self.pair.local.port(), self.pair.remote.port(), 0, 0);
        tcph.acknowledgment_number = seg.sequence_number() + seg_len;
        tcph.rst = true;
        tcph.ack = true;

        self.send_datagram(dev, tcph, &[])
    }

    pub fn process_estab(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
    ) -> TcbResult<()> {
        if seg.rst() {
            self.send_rst(dev, seg.sequence_number()).unwrap();
            return Err(ConnectionError::ConnectionReset);
        }
        if seg.fin() && seg.ack() {
            println!("connection closing");
            self.send_ack(dev, seg).unwrap();
            self.state = State::CloseWait;
            return Ok(());
        }

        let seg_ack = seg.acknowledgment_number();
        if self.snd_una < seg_ack && seg_ack <= self.snd_nxt {
            self.snd_una = seg_ack;
        }
        println!("Ready for data exchange!");

        // todo!();

        Ok(())
    }

    pub fn process_close(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
    ) -> TcbResult<()> {
        if !seg.rst() {
            match seg.ack() {
                true => {
                    self.send_rst(dev, seg.sequence_number()).unwrap();
                }
                false => {
                    self.send_rst_ack(dev, seg).unwrap();
                }
            }
        }
        Ok(())
    }

    pub fn send_ack(
        &mut self,
        dev: &mut TunDevice,
        seg: etherparse::TcpHeaderSlice,
    ) -> std::io::Result<()> {
        let ws = self.window().expect("idk man");
        let mut tcph = etherparse::TcpHeader::new(
            self.pair.local.port(),
            self.pair.remote.port(),
            self.snd_nxt,
            ws,
        );
        tcph.acknowledgment_number = seg.sequence_number() + 1;
        tcph.ack = true;

        self.send_datagram(dev, tcph, &[])
    }

    // Update window size based on the rx buffer
    pub fn window(&self) -> Option<u16> {
        let ws = self.rx_buffer.capacity() - self.rx_buffer.len();
        ws.try_into().ok()
    }
}
