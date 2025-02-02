use std::{collections::VecDeque, fmt::Display, io::Write, net::SocketAddrV4};

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

// Stands for Transmission Control Block
#[derive(Hash, Eq, PartialEq, Debug)]
pub struct Tcb {
    state: State,
    pair: SocketPair,
    tx_buffer: VecDeque<u8>,
    rx_buffer: VecDeque<u8>,
    snd_una: u32,
    snd_nxt: u32,
    snd_wnd: u32,
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
            iss: rand::random::<u32>(),
            rcv_nxt: 0,
            rcv_wnd: 0,
            irs: 0,
        }
    }

    pub fn poll(&mut self, dev: &mut TunDevice, seg: etherparse::TcpHeaderSlice) {
        match self.state {
            State::Listen => {
                self.process_listen(dev, seg);
            }
            State::SynRcvd => {
                self.process_syn_rcvd(dev, seg);
            }
            State::Estab => {
                println!("{}", self.state);
                self.process_estab(dev, seg);
            }
            State::FinWait1 => {
                println!("{}", self.state);
            }
            State::FinWait2 => {
                println!("{}", self.state);
            }
            State::CloseWait => {
                println!("{}", self.state);
            }
            State::Closing => {
                println!("{}", self.state);
            }
            State::TimeWait => {
                println!("{}", self.state);
            }
            State::Closed => {
                println!("{}", self.state);
                if !seg.rst() || seg.ack() {
                    self.send_rst(dev, seg.sequence_number());
                    return;
                }
                self.send_rst_ack(dev, seg);
            }
            State::SynSent => {
                self.process_syn_sent(dev, seg);
            }
            _ => {}
        }
    }

    pub fn process_listen(&mut self, dev: &mut TunDevice, seg: etherparse::TcpHeaderSlice) {
        if seg.rst() {
            return;
        }
        if seg.ack() {
            self.send_rst(dev, seg.acknowledgment_number());
            return;
        }

        // Security and precedence checks are skipped
        if seg.syn() {
            self.rcv_nxt = seg.sequence_number() + 1;
            self.irs = seg.sequence_number();
            // Any other control or text should be queued for processing later.

            let ws = seg.window_size();
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

            let builder = etherparse::PacketBuilder::ipv4(
                self.pair.local.ip().octets(),
                self.pair.remote.ip().octets(),
                HOP_LIMIT,
            )
            .tcp_header(tcph);

            builder.write(&mut self.tx_buffer, &[]).unwrap();
        }

        let _ = dev.write_all(self.tx_buffer.make_contiguous());
        self.tx_buffer.clear();
    }

    pub fn process_syn_sent(&mut self, dev: &mut TunDevice, seg: etherparse::TcpHeaderSlice) {
        let seg_ack = seg.acknowledgment_number();
        if seg_ack <= self.iss || seg_ack > self.snd_nxt {
            if seg.rst() {
                return;
            }
            self.send_rst(dev, seg_ack);
            return;
        }

        match seg_ack >= self.snd_una && seg_ack <= self.snd_nxt {
            true => {
                if seg.rst() {
                    self.state = State::Closed;
                    // TODO: "error: connection reset"
                    todo!();
                }
            }
            false => return,
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

                let builder = etherparse::PacketBuilder::ipv4(
                    self.pair.local.ip().octets(),
                    self.pair.remote.ip().octets(),
                    HOP_LIMIT,
                )
                .tcp_header(tcph);

                builder.write(&mut self.tx_buffer, &[]).unwrap();
            }
        }

        let _ = dev.write_all(self.tx_buffer.make_contiguous());
        self.tx_buffer.clear();
    }

    pub fn process_syn_rcvd(&mut self, dev: &mut TunDevice, seg: etherparse::TcpHeaderSlice) {
        if seg.rst() {
            self.state = State::Listen;
            return;
        }

        let seg_ack = seg.acknowledgment_number();
        match seg_ack >= self.snd_una && seg_ack <= self.snd_nxt {
            true => {
                if seg.rst() {
                    self.state = State::Closed;
                    // TODO: "error: connection reset"
                    todo!();
                }
                self.state = State::Estab;
            }
            false => {
                self.send_rst(dev, seg.sequence_number());
            }
        }
    }

    pub fn send_rst(&mut self, dev: &mut TunDevice, seq: u32) {
        let mut tcph =
            etherparse::TcpHeader::new(self.pair.local.port(), self.pair.remote.port(), seq, 0);
        tcph.rst = true;

        let builder = etherparse::PacketBuilder::ipv4(
            self.pair.local.ip().octets(),
            self.pair.remote.ip().octets(),
            HOP_LIMIT,
        )
        .tcp_header(tcph);
        builder.write(&mut self.tx_buffer, &[]).unwrap();

        let _ = dev.write_all(self.tx_buffer.make_contiguous());
        self.tx_buffer.clear();
    }

    pub fn send_rst_ack(&mut self, dev: &mut TunDevice, seg: etherparse::TcpHeaderSlice) {
        // <SEQ=0><ACK=SEG.SEQ+SEG.LEN><CTL=RST,ACK>
        let seg_len: u32 = seg.slice().len().try_into().unwrap();
        let mut tcph =
            etherparse::TcpHeader::new(self.pair.local.port(), self.pair.remote.port(), 0, 0);
        tcph.acknowledgment_number = seg.sequence_number() + seg_len;
        tcph.rst = true;
        tcph.ack = true;

        let builder = etherparse::PacketBuilder::ipv4(
            self.pair.local.ip().octets(),
            self.pair.remote.ip().octets(),
            HOP_LIMIT,
        )
        .tcp_header(tcph);
        builder.write(&mut self.tx_buffer, &[]).unwrap();

        let _ = dev.write_all(self.tx_buffer.make_contiguous());
        self.tx_buffer.clear();
    }

    pub fn process_estab(&mut self, dev: &mut TunDevice, seg: etherparse::TcpHeaderSlice) {
        if seg.rst() {
            self.send_rst(dev, seg.sequence_number());
            self.state = State::Closed;
            return;
        }
        if seg.fin() && seg.ack() {
            println!("Someone is trying to gracefully shutdown.");
            self.state = State::CloseWait;
            self.send_ack(dev, seg);
            return;
        }
        let seg_ack = seg.acknowledgment_number();
        if self.snd_una < seg_ack && seg_ack <= self.snd_nxt {
            self.snd_una = seg_ack;
        }
        println!("Ready for data exchange!");
    }

    pub fn accesability_test() {
        // Segment Receive  Test
        // Length  Window
        // ------- -------  -------------------------------------------
        //
        //    0       0     SEG.SEQ = RCV.NXT
        //
        //    0      >0     RCV.NXT =< SEG.SEQ < RCV.NXT+RCV.WND
        //
        //   >0       0     not acceptable
        //
        //   >0      >0     RCV.NXT =< SEG.SEQ < RCV.NXT+RCV.WND
        //                  or RCV.NXT =< SEG.SEQ+SEG.LEN-1 < RCV.NXT+RCV.WND
        todo!()
    }

    pub fn send_ack(&mut self, dev: &mut TunDevice, seg: etherparse::TcpHeaderSlice) {
        let mut tcph = etherparse::TcpHeader::new(
            self.pair.local.port(),
            self.pair.remote.port(),
            self.snd_nxt,
            seg.window_size(),
        );
        tcph.acknowledgment_number = seg.sequence_number() + 1;
        tcph.ack = true;

        let builder = etherparse::PacketBuilder::ipv4(
            self.pair.local.ip().octets(),
            self.pair.remote.ip().octets(),
            HOP_LIMIT,
        )
        .tcp_header(tcph);

        builder.write(&mut self.tx_buffer, &[]).unwrap();

        let _ = dev.write_all(self.tx_buffer.make_contiguous());
        self.tx_buffer.clear();
    }
}
