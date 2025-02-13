use std::{collections::VecDeque, io, net::SocketAddrV4};

/// The state of a TCB, according to RFC 793.
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

pub enum PollResult {
    SendDatagram(Vec<u8>),
    TcbError(io::Error),
    ShouldRetransmit,
    NoAction,
}

/// TTL for IPv4
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

/// Transmission Control Block
#[derive(Hash, Eq, PartialEq, Debug)]
pub struct Tcb {
    /// TCB state
    state: State,
    /// Connection pair
    pair: SocketPair,
    /// Determines whether it's a client or a server
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
    rcv_wnd: u16,
    /// Used for urgent data
    rcv_up: u32,
    /// Lost segments detection timeout
    rto: Option<std::time::Duration>,
}

impl Tcb {
    pub fn new(pair: SocketPair) -> Self {
        Self {
            state: State::Listen,
            pair,
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
            rcv_wnd: 4096,
            rcv_up: 0,
            rto: None,
        }
    }

    pub fn is_acceptable(&self, seg: &etherparse::TcpHeaderSlice, len: usize) -> bool {
        // Length  Window        Test
        // ------- -------  -------------------------------------------
        //  0        0     SEG.SEQ = RCV.NXT
        //
        //  0       >0     RCV.NXT =< SEG.SEQ < RCV.NXT+RCV.WND
        //
        // >0        0     not acceptable
        //
        //  >0      >0     RCV.NXT =< SEG.SEQ < RCV.NXT+RCV.WND
        //              or RCV.NXT =< SEG.SEQ+SEG.LEN-1 < RCV.NXT+RCV.WND

        let seg_seq = seg.sequence_number();
        let seg_len = Self::segment_length(&seg, len);
        let seg_end = seg_seq.wrapping_add(seg_len - 1);
        let rcv_win = self.rcv_nxt + self.rcv_wnd as u32;

        match (seg_len, self.rcv_wnd) {
            (0, 0) => {
                if seg_seq == self.rcv_nxt {
                    return true;
                }
            }
            (0, window) if window > 0 => {
                if self.rcv_nxt <= seg_seq && seg_seq < rcv_win {
                    return true;
                }
            }
            (length, 0) if length > 0 => return false,
            (length, window) if length > 0 && window > 0 => {
                if self.rcv_nxt <= seg_seq && seg_seq < rcv_win
                    || self.rcv_nxt <= seg_end && seg_end < rcv_win
                {
                    return true;
                }
            }
            _ => {
                unreachable!()
            }
        }
        false
    }

    pub fn write(&self, tcph: etherparse::TcpHeader, payload: &[u8]) -> PollResult {
        let builder = etherparse::PacketBuilder::ipv4(
            self.pair.local.ip().octets(),
            self.pair.remote.ip().octets(),
            HOP_LIMIT,
        )
        .tcp_header(tcph);

        let mut datagram: Vec<u8> = Vec::<u8>::with_capacity(builder.size(payload.len()));

        match builder.write(&mut datagram, payload) {
            Ok(_) => PollResult::SendDatagram(datagram),
            Err(_) => PollResult::TcbError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Packet serialization failed",
            )),
        }
    }

    pub fn poll(&mut self, seg: etherparse::TcpHeaderSlice, payload: &[u8]) -> PollResult {
        if !matches!(self.state, State::Listen | State::SynSent)
            && !self.is_acceptable(&seg, payload.len())
        {
            return self.build_ack();
        }

        match self.state {
            State::Listen => self.listen(seg),
            State::SynRcvd => self.syn_rcvd(seg),
            State::Estab => self.estab(seg, payload),
            State::Closed => self.close(seg, payload),
            State::SynSent => self.syn_sent(seg),
            State::CloseWait => self.close_wait(seg),
            _ => PollResult::NoAction,
        }
    }

    pub fn listen(&mut self, seg: etherparse::TcpHeaderSlice) -> PollResult {
        // RST should be ignored in LISTEN state
        if seg.rst() {
            return PollResult::NoAction;
        }
        if seg.ack() {
            return self.build_rst(seg.acknowledgment_number());
        }

        // Security and precedence checks are skipped
        if seg.syn() {
            self.irs = seg.sequence_number();
            self.rcv_nxt = seg.sequence_number().wrapping_add(1);

            let mut tcph = etherparse::TcpHeader::new(
                self.pair.local.port(),
                self.pair.remote.port(),
                self.iss,
                self.rx_window(),
            );
            tcph.acknowledgment_number = self.rcv_nxt;
            tcph.syn = true;
            tcph.ack = true;

            self.snd_una = self.iss;
            self.snd_nxt = self.iss.wrapping_add(1);

            self.state = State::SynRcvd;

            return self.write(tcph, &[]);
        }

        PollResult::NoAction
    }

    pub fn syn_rcvd(&mut self, seg: etherparse::TcpHeaderSlice) -> PollResult {
        if seg.rst() {
            if self.connection_type == ConnectionType::Passive {
                self.state = State::Listen;
                return PollResult::NoAction;
            } else {
                self.tx_buffer.clear();
                return PollResult::TcbError(io::Error::from(io::ErrorKind::ConnectionReset));
            }
        }

        if seg.ack() {
            let seg_ack = seg.acknowledgment_number();
            match seg_ack > self.snd_una && seg_ack <= self.snd_nxt {
                true => {
                    if seg.rst() {
                        return PollResult::TcbError(io::Error::from(
                            io::ErrorKind::ConnectionReset,
                        ));
                    }
                    self.state = State::Estab;
                    println!("accepted a connection from: {}", self.pair.remote);
                }
                false => {
                    self.build_rst(seg.sequence_number());
                }
            }
        }

        PollResult::NoAction
    }

    pub fn syn_sent(&mut self, seg: etherparse::TcpHeaderSlice) -> PollResult {
        let seg_ack = seg.acknowledgment_number();
        if seg_ack <= self.iss || seg_ack > self.snd_nxt {
            if seg.rst() {
                return PollResult::NoAction;
            }
            return self.build_rst(seg_ack);
        }

        match seg_ack >= self.snd_una && seg_ack <= self.snd_nxt {
            true => {
                if seg.rst() {
                    return PollResult::TcbError(io::Error::from(io::ErrorKind::ConnectionReset));
                }
            }
            false => return PollResult::NoAction,
        }

        if seg.syn() {
            self.rcv_nxt = seg.sequence_number() + 1;
            self.irs = seg.sequence_number();
            if seg.ack() {
                self.snd_una = seg_ack;
            }

            if self.snd_una > self.iss {
                self.state = State::Estab;

                let mut tcp_header = etherparse::TcpHeader::new(
                    self.pair.local.port(),
                    self.pair.remote.port(),
                    self.snd_nxt,
                    self.rx_window(),
                );
                tcp_header.acknowledgment_number = self.rcv_nxt;
                tcp_header.ack = true;

                return self.write(tcp_header, &[]);
            }
        }

        PollResult::NoAction
    }

    pub fn build_rst(&mut self, seq: u32) -> PollResult {
        let mut tcph =
            etherparse::TcpHeader::new(self.pair.local.port(), self.pair.remote.port(), seq, 0);
        tcph.rst = true;

        self.write(tcph, &[])
    }

    pub fn send_rst_ack(&mut self, seq: u32, seg_len: u32) -> PollResult {
        // <SEQ=0><ACK=SEG.SEQ+SEG.LEN><CTL=RST,ACK>
        let mut tcph =
            etherparse::TcpHeader::new(self.pair.local.port(), self.pair.remote.port(), 0, 0);
        tcph.acknowledgment_number = seq.wrapping_add(seg_len);
        tcph.rst = true;
        tcph.ack = true;

        self.write(tcph, &[])
    }

    pub fn estab(&mut self, seg: etherparse::TcpHeaderSlice, payload: &[u8]) -> PollResult {
        if seg.rst() {
            return PollResult::TcbError(io::Error::from(io::ErrorKind::ConnectionReset));
        }

        if seg.psh() && !payload.is_empty() {
            self.rx_buffer.extend(payload);
            println!("Received a message: {}", String::from_utf8_lossy(payload));
            self.rcv_nxt = self.rcv_nxt.wrapping_add(payload.len() as u32);
            return self.build_ack();
        }

        if seg.ack() {
            let seg_ack = seg.acknowledgment_number();
            if self.snd_una < seg_ack && seg_ack <= self.snd_nxt {
                println!("removing acknowledged octets from tx...");
                self.snd_una = seg_ack;
                // TODO: remove acknowledged seqs from tx_buffer.
            }
        }

        // Remote socket won't send anymore!
        if seg.fin() {
            println!("connection closing");
            self.state = State::CloseWait;

            self.rcv_nxt = self.rcv_nxt.wrapping_add(1);

            self.build_ack();

            // TODO: FIN implies PUSH for any segment text not yet delivered to the user.
        }

        PollResult::NoAction
    }

    pub fn close(&mut self, seg: etherparse::TcpHeaderSlice, payload: &[u8]) -> PollResult {
        if !seg.rst() {
            match seg.ack() {
                true => return self.build_rst(seg.sequence_number()),
                false => return self.send_rst_ack(seg.sequence_number(), payload.len() as u32),
            }
        }
        PollResult::NoAction
    }

    pub fn close_wait(&mut self, _seg: etherparse::TcpHeaderSlice) -> PollResult {
        // TODO: send [FIN,ACK]
        self.state = State::LastAck;
        PollResult::NoAction
    }

    pub fn build_ack(&mut self) -> PollResult {
        let mut tcph = etherparse::TcpHeader::new(
            self.pair.local.port(),
            self.pair.remote.port(),
            self.snd_nxt,
            self.rx_window(),
        );
        tcph.acknowledgment_number = self.rcv_nxt;
        tcph.ack = true;

        self.write(tcph, &[])
    }

    pub fn segment_length(seg: &etherparse::TcpHeaderSlice, len: usize) -> u32 {
        let mut seg_len = len as u32;

        if seg.fin() {
            seg_len += 1;
        }
        if seg.syn() {
            seg_len += 1;
        }

        seg_len
    }

    pub fn rx_window(&self) -> u16 {
        (self.rx_buffer.capacity() - self.rx_buffer.len()) as u16
    }
}
