use std::{
    collections::VecDeque,
    io::{self, Write},
    net::SocketAddrV4,
};

use crate::device;

/// The state of a TCB
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
    pub(crate) tx_buffer: VecDeque<u8>,
    /// Receive buffer
    pub(crate) rx_buffer: VecDeque<u8>,
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
        }
    }

    pub(crate) fn poll(
        &mut self,
        dev: &mut device::TunDevice,
        hdr: etherparse::TcpHeaderSlice,
        payload: &[u8],
    ) -> io::Result<()> {
        // Try to establish a connection
        match self.state {
            State::Listen => {
                return self.process_listen(dev, hdr);
            }
            State::SynSent => {
                return self.process_syn_sent(dev, hdr);
            }
            State::Closed => {
                return self.process_close(dev, hdr, payload);
            }
            _ => {}
        }

        // check sequence number
        if !matches!(self.state, State::Listen | State::SynSent | State::Closed)
            && !self.is_acceptable(&hdr, payload.len())
        {
            self.write_ack(dev)?;
        }

        // check the RST bit
        if hdr.rst() {
            match self.state {
                State::SynRcvd => {
                    if self.connection_type == ConnectionType::Passive {
                        self.state = State::Listen;
                        return Ok(());
                    } else {
                        self.tx_buffer.clear();
                        return Err(io::Error::from(io::ErrorKind::ConnectionReset));
                    }
                }
                State::Estab | State::FinWait1 | State::FinWait2 | State::CloseWait => {
                    // Any outstanding RECEIVEs and SEND should receive "reset" responses.
                    // All segment queues should be flushed. Users should also receive an unsolicited general
                    // "connection reset" signal. Enter the CLOSED state, delete the
                    //TCB, and return.
                    return Err(io::Error::from(io::ErrorKind::ConnectionReset));
                }
                State::Closing | State::LastAck | State::TimeWait => {
                    return Err(io::Error::from(io::ErrorKind::ConnectionReset));
                }
                _ => {}
            }
        }

        // check security and precedence

        // check the SYN bit
        if hdr.syn() {
            if !matches!(self.state, State::Closed | State::SynSent) {
                // If the SYN is in the window it is an error, send a reset, any
                // outstanding RECEIVEs and SEND should receive "reset" responses,
                // all segment queues should be flushed, the user should also
                // receive an unsolicited general "connection reset" signal, enter
                // the CLOSED state, delete the TCB, and return.
                //
                // If the SYN is not in the window self step would not be reached
                // and an ack would have been sent in the first step (sequence
                // number check).
            }
        }

        if hdr.ack() {
            let seg_ack = hdr.acknowledgment_number();
            let seg_seq = hdr.sequence_number();
            let seg_wnd = hdr.window_size();
            match self.state {
                State::SynRcvd => match seg_ack > self.snd_una && seg_ack <= self.snd_nxt {
                    true => {
                        if hdr.rst() {
                            return Err(io::Error::from(io::ErrorKind::ConnectionReset));
                        }
                        self.state = State::Estab;
                        println!("accepted a connection from: {}", self.pair.remote);
                    }
                    false => {
                        self.write_rst(dev, hdr.sequence_number())?;
                    }
                },
                State::Estab | State::CloseWait => {
                    if self.snd_una < seg_ack && seg_ack <= self.snd_nxt {
                        self.snd_una = seg_ack;

                        // TODO: remove acknowledged seqs from tx_buffer.
                        let una_index = (self.snd_una - self.iss - 1) as usize;
                        println!(
                            "una_index: {}, tx_buffer len: {}",
                            una_index,
                            self.tx_buffer.len()
                        );

                        // Remove everything up to SND.UNA
                        self.tx_buffer.drain(0..una_index.min(self.tx_buffer.len()));

                        // Updating the window from send sequence space
                        if self.snd_wl1 < seg_seq
                            || (self.snd_wl1 == seg_seq && self.snd_wl2 <= seg_ack)
                        {
                            self.snd_wnd = seg_wnd;
                            self.snd_wl1 = seg_seq;
                            self.snd_wl2 = seg_ack;
                        }
                    }

                    if seg_ack > self.snd_una {
                        // If the ACK is duplicate it can be ignored
                        println!("The ACK is duplicate");
                        return Ok(());
                    }

                    // If the ACK acks something not yet sent
                    if seg_ack > self.snd_nxt {
                        println!("ACKing something not yet sent");
                        return self.write_ack(dev);
                    }
                }
                State::FinWait1 => {
                    // TODO:
                    // In addition to the processing for the ESTABLISHED state, if
                    // our FIN is now acknowledged then enter FIN-WAIT-2 and continue
                    // processing in that state.
                    self.state = State::FinWait2;
                }
                State::FinWait2 => {
                    // TODO:
                    // In addition to the processing for the ESTABLISHED state, if
                    // the retransmission queue is empty, the user's CLOSE can be
                    // acknowledged ("ok") but do not delete the TCB.
                }
                State::Closing => {
                    // TODO:
                    // In addition to the processing for the ESTABLISHED state, if
                    // the ACK acknowledges our FIN then enter the TIME-WAIT state,
                    // otherwise ignore the segment.
                    self.state = State::TimeWait;
                }
                State::LastAck => {
                    // TODO:
                    // The only thing that can arrive in self state is an
                    // acknowledgment of our FIN.  If our FIN is now acknowledged,
                    // delete the TCB, enter the CLOSED state, and return.
                }
                State::TimeWait => {
                    // TODO:
                    // The only thing that can arrive in self state is a
                    // retransmission of the remote FIN.  Acknowledge it, and restart
                    // the 2 MSL timeout.
                }
                _ => {}
            }
        } else {
            return Ok(());
        }

        if hdr.urg() {
            unimplemented!()
        }

        if let State::Estab | State::FinWait1 | State::FinWait2 = self.state {
            // process the segment text
            if !payload.is_empty() {
                self.rx_buffer.extend(payload);

                println!("Received a message: {}", String::from_utf8_lossy(payload));

                self.rcv_nxt = self.rcv_nxt.wrapping_add(payload.len() as u32);
                self.rcv_wnd = self.rx_window();

                self.write_ack(dev)?;
            }

            if !self.tx_buffer.is_empty() {
                let mut th = etherparse::TcpHeader::new(
                    self.pair.local.port(),
                    self.pair.remote.port(),
                    self.snd_nxt,
                    self.rcv_wnd,
                );
                th.acknowledgment_number = self.rcv_nxt;
                th.psh = true;
                th.ack = true;

                let (first, _second) = self.tx_buffer.as_slices();
                self.write_all(dev, th, first)?;
                // TODO: start the RTO

                // When the sender creates a segment and transmits it the sender advances SND.NXT
                self.snd_nxt = self.snd_nxt.wrapping_add(first.len() as u32);
            }
        }

        // TODO: eighth, check the FIN bit
        if hdr.fin() {
            // SEG.SEQ cannot be validated in CLOSED, LISTEN or SYN-SENT
            if !matches!(self.state, State::Closed | State::Listen | State::SynSent) {
                println!("Connection closing");

                self.state = State::CloseWait;

                self.rcv_nxt = self.rcv_nxt.wrapping_add(1);

                self.write_ack(dev)?;

                match self.state {
                    State::SynRcvd | State::Estab => {
                        self.state = State::CloseWait;
                    }
                    State::FinWait1 => {
                        // TODO:
                        // If our FIN has been ACKed (perhaps in this segment), then
                        // enter TIME-WAIT, start the time-wait timer, turn off the other
                        // timers; otherwise enter the CLOSING state.
                    }
                    State::FinWait2 => {
                        // TODO:
                        //Enter the TIME-WAIT state.  Start the time-wait timer, turn
                        // off the other timers.
                    }
                    State::TimeWait => {
                        // TODO:
                        // Remain in the TIME-WAIT state.  Restart the 2 MSL time-wait
                        // timeout and return.
                    }

                    // Remain in other states
                    _ => {}
                }
            }
        }

        Ok(())
    }

    fn process_listen(
        &mut self,
        dev: &mut device::TunDevice,
        hdr: etherparse::TcpHeaderSlice,
    ) -> io::Result<()> {
        // RST should be ignored in LISTEN state
        if hdr.rst() {
            return Ok(());
        }
        if hdr.ack() {
            return self.write_rst(dev, hdr.acknowledgment_number());
        }

        // Security and precedence checks are skipped
        if hdr.syn() {
            self.irs = hdr.sequence_number();
            self.rcv_nxt = hdr.sequence_number().wrapping_add(1);
            self.rcv_wnd = self.rx_window();

            let mut th = etherparse::TcpHeader::new(
                self.pair.local.port(),
                self.pair.remote.port(),
                self.iss,
                self.rcv_wnd,
            );
            th.acknowledgment_number = self.rcv_nxt;
            th.syn = true;
            th.ack = true;

            self.snd_una = self.iss;
            self.snd_nxt = self.iss.wrapping_add(1);

            self.state = State::SynRcvd;

            return self.write_all(dev, th, &[]);
        }

        Ok(())
    }

    fn process_syn_sent(
        &mut self,
        dev: &mut device::TunDevice,
        hdr: etherparse::TcpHeaderSlice,
    ) -> io::Result<()> {
        let seg_ack = hdr.acknowledgment_number();
        if seg_ack <= self.iss || seg_ack > self.snd_nxt {
            if hdr.rst() {
                return Ok(());
            }
            return self.write_rst(dev, seg_ack);
        }

        match seg_ack >= self.snd_una && seg_ack <= self.snd_nxt {
            true => {
                if hdr.rst() {
                    return Err(io::Error::from(io::ErrorKind::ConnectionReset));
                }
            }
            false => return Ok(()),
        }

        if hdr.syn() {
            self.rcv_nxt = hdr.sequence_number() + 1;
            self.irs = hdr.sequence_number();
            if hdr.ack() {
                self.snd_una = seg_ack;
            }

            if self.snd_una > self.iss {
                self.state = State::Estab;

                let mut th = etherparse::TcpHeader::new(
                    self.pair.local.port(),
                    self.pair.remote.port(),
                    self.snd_nxt,
                    self.rx_window(),
                );
                th.acknowledgment_number = self.rcv_nxt;
                th.ack = true;

                return self.write_all(dev, th, &[]);
            }
        }

        Ok(())
    }

    fn process_close(
        &mut self,
        dev: &mut device::TunDevice,
        hdr: etherparse::TcpHeaderSlice,
        payload: &[u8],
    ) -> io::Result<()> {
        if !hdr.rst() {
            match hdr.ack() {
                true => return self.write_rst(dev, hdr.sequence_number()),
                false => {
                    return self.write_rst_ack(dev, hdr.sequence_number(), payload.len() as u32)
                }
            }
        }

        Ok(())
    }

    fn segment_length(hdr: &etherparse::TcpHeaderSlice, len: usize) -> u32 {
        let mut seg_len = len as u32;

        if hdr.fin() {
            seg_len += 1;
        }
        if hdr.syn() {
            seg_len += 1;
        }

        seg_len
    }

    fn is_acceptable(&self, hrd: &etherparse::TcpHeaderSlice, len: usize) -> bool {
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

        let seg_seq = hrd.sequence_number();
        let seg_len = Self::segment_length(hrd, len);
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

    fn rx_window(&self) -> u16 {
        (self.rx_buffer.capacity() - self.rx_buffer.len()) as u16
    }

    fn write_ack(&mut self, dev: &mut device::TunDevice) -> io::Result<()> {
        let mut th = etherparse::TcpHeader::new(
            self.pair.local.port(),
            self.pair.remote.port(),
            self.snd_nxt,
            self.rcv_wnd,
        );
        th.acknowledgment_number = self.rcv_nxt;
        th.ack = true;

        self.write_all(dev, th, &[])
    }

    fn write_rst(&mut self, dev: &mut device::TunDevice, seq: u32) -> io::Result<()> {
        let mut th =
            etherparse::TcpHeader::new(self.pair.local.port(), self.pair.remote.port(), seq, 0);
        th.rst = true;

        self.write_all(dev, th, &[])
    }

    fn write_rst_ack(
        &mut self,
        dev: &mut device::TunDevice,
        seq: u32,
        seg_len: u32,
    ) -> io::Result<()> {
        // <SEQ=0><ACK=SEG.SEQ+SEG.LEN><CTL=RST,ACK>
        let mut th =
            etherparse::TcpHeader::new(self.pair.local.port(), self.pair.remote.port(), 0, 0);
        th.acknowledgment_number = seq.wrapping_add(seg_len);
        th.rst = true;
        th.ack = true;

        self.write_all(dev, th, &[])
    }

    fn write_all(
        &self,
        dev: &mut device::TunDevice,
        hdr: etherparse::TcpHeader,
        payload: &[u8],
    ) -> io::Result<()> {
        let builder = etherparse::PacketBuilder::ipv4(
            self.pair.local.ip().octets(),
            self.pair.remote.ip().octets(),
            HOP_LIMIT,
        )
        .tcp_header(hdr);

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
