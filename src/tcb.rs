use std::{
    collections::{BTreeMap, VecDeque},
    io::{self, Write},
    net::SocketAddrV4,
    time::{Duration, Instant},
};

use crate::device;

#[derive(Default)]
struct TcpFlags {
    syn: bool,
    psh: bool,
    rst: bool,
}

#[derive(Hash, PartialEq, Eq, Debug)]
struct TimerEntry {
    expires_at: Instant,
    snd_nxt: u32,
    payload_len: usize,
}

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
pub struct ConnectionPair {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
}

#[derive(Eq, PartialEq, Debug)]
pub enum ConnectionType {
    Active,
    Passive,
}

/// Transmission Control Block
pub struct Tcb {
    /// TCB state
    state: State,
    /// Local address specified with listen()
    listen_addr: SocketAddrV4,
    /// Remote address obtained in listen
    remote_addr: Option<SocketAddrV4>,
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
    /// RTO in (ms)
    rto: Duration,
    /// Timeouts for the current connection
    timers: BTreeMap<u32, TimerEntry>,
}

impl Tcb {
    pub fn new(addr: SocketAddrV4) -> Self {
        Self {
            state: State::Closed,
            listen_addr: addr,
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
            rto: Duration::from_millis(200),
            timers: BTreeMap::new(),
            remote_addr: None,
        }
    }

    pub fn listen_addr(&self) -> SocketAddrV4 {
        self.listen_addr
    }

    pub fn remote_addr(&self) -> Option<SocketAddrV4> {
        self.remote_addr
    }

    pub fn listen(&mut self) {
        self.state = State::Listen;
    }

    // half-establish a connection
    pub fn try_accept(
        &mut self,
        dev: &mut device::TunDevice,
        hdr: &etherparse::TcpHeaderSlice,
        cp: ConnectionPair,
    ) -> io::Result<Option<Tcb>> {
        if self.state != State::Listen {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "attempt to accept while not in listen state",
            ));
        }
        if hdr.rst() {
            return Ok(None);
        }
        /* security and precedence checks are skipped */
        let mut sock = Tcb::new(cp.local);
        sock.remote_addr = Some(cp.remote);
        if hdr.ack() {
            sock.write_rst(dev, hdr.acknowledgment_number())?;
        }
        if hdr.syn() {
            sock.connection_type = ConnectionType::Passive;
            sock.irs = hdr.sequence_number();
            sock.rcv_nxt = hdr.sequence_number().wrapping_add(1);
            sock.rcv_wnd = sock.rx_window();
            sock.snd_una = sock.iss;
            sock.snd_nxt = sock.iss.wrapping_add(1);
            sock.state = State::SynRcvd;

            let flags = TcpFlags {
                syn: true,
                ..Default::default()
            };
            println!("sending syn, ack");
            sock.write_all(dev, sock.iss, Some(sock.rcv_nxt), flags, &[])?;
            return Ok(Some(sock));
        }
        Ok(None)
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        todo!()
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.rx_buffer.is_empty() {
            return Ok(0);
        }
        let available = self.rx_buffer.len();
        let to_read = std::cmp::min(buf.len(), available);
        let drained = self.rx_buffer.drain(..to_read).collect::<Vec<u8>>();
        buf[..to_read].copy_from_slice(&drained);
        Ok(to_read)
    }

    pub fn on_tick(&mut self, dev: &mut device::TunDevice) -> io::Result<()> {
        let now = Instant::now();

        if let Some((&snd_una, _)) = self
            .timers
            .iter()
            .find(|(_, timer)| timer.expires_at <= now)
        {
            let timer = self.timers.remove(&snd_una).unwrap();

            let to_write: Vec<u8> = self
                .tx_buffer
                .range(0..timer.payload_len)
                .copied()
                .collect();

            println!(
                "retransmitting: {:?}",
                String::from_utf8_lossy(to_write.as_slice())
            );

            let flags = TcpFlags {
                psh: true,
                ..Default::default()
            };

            self.write_all(
                dev,
                timer.snd_nxt,
                Some(self.rcv_nxt),
                flags,
                to_write.as_slice(),
            )?;

            // exponential back-off
            self.rto *= 2;

            // Restart the timer
            self.timers.insert(
                snd_una,
                TimerEntry {
                    expires_at: Instant::now() + self.rto,
                    snd_nxt: timer.snd_nxt,
                    payload_len: timer.payload_len,
                },
            );
        }
        Ok(())
    }

    pub(crate) fn on_segment(
        &mut self,
        dev: &mut device::TunDevice,
        tcph: &etherparse::TcpHeaderSlice,
        payload: &[u8],
    ) -> io::Result<()> {
        // Try to establish a connection
        match self.state {
            State::SynSent => {
                return self.process_syn_sent(dev, tcph);
            }
            State::Closed => {
                return self.process_close(dev, tcph, payload);
            }
            _ => {}
        }

        // check sequence number
        if !matches!(self.state, State::Listen | State::SynSent | State::Closed)
            && !self.is_acceptable(&tcph, payload.len())
        {
            self.write_ack(dev)?;
        }

        // check the RST bit
        if tcph.rst() {
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
        if tcph.syn() {
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

        if tcph.ack() {
            let seg_ack = tcph.acknowledgment_number();
            let seg_seq = tcph.sequence_number();
            let seg_wnd = tcph.window_size();

            match self.state {
                State::SynRcvd => match seg_ack > self.snd_una && seg_ack <= self.snd_nxt {
                    true => {
                        if tcph.rst() {
                            return Err(io::Error::from(io::ErrorKind::ConnectionReset));
                        }
                        self.state = State::Estab;
                        let msg = format!("Hello, {}", self.remote_addr().unwrap());
                        self.tx_buffer.extend(msg.into_bytes());
                    }
                    false => {
                        self.write_rst(dev, tcph.sequence_number())?;
                    }
                },
                State::Estab | State::CloseWait => {
                    if self.snd_una < seg_ack && seg_ack <= self.snd_nxt {
                        println!("received ACK, shifting snd_una...");
                        self.snd_una = seg_ack;

                        // it's possible that not everything was acknowledged
                        let ack_idx = (seg_ack - self.iss - 1) as usize;

                        println!(
                            "ack_index: {}, tx_buffer len: {}",
                            ack_idx,
                            self.tx_buffer.len()
                        );

                        // remove everything up to seg_ack
                        self.tx_buffer.drain(0..ack_idx.min(self.tx_buffer.len()));

                        // cancel the retransmit timer associated with the snd_una
                        if let Some((&key, _)) =
                            self.timers.iter().find(|(&snd_una, _)| snd_una <= seg_ack)
                        {
                            self.timers.remove(&key).unwrap();
                            self.rto = Duration::from_millis(200);
                            println!("canceled RTO for: {}", key);
                        }

                        // updating the window from send sequence space
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

        if tcph.urg() {
            unimplemented!()
        }

        if let State::Estab | State::FinWait1 | State::FinWait2 = self.state {
            // process the segment text
            if !payload.is_empty() {
                self.rx_buffer.extend(payload);

                self.rcv_nxt = self.rcv_nxt.wrapping_add(payload.len() as u32);
                self.rcv_wnd = self.rx_window();

                self.write_ack(dev)?;
            }

            // transmitting data
            if !self.tx_buffer.is_empty() {
                // TODO: fix this embarrassment
                let (head, _) = self.tx_buffer.as_slices();

                let flags = TcpFlags {
                    psh: true,
                    ..Default::default()
                };

                self.write_all(dev, self.snd_nxt, Some(self.rcv_nxt), flags, head)?;

                println!(
                    "sending data: {:?}\nsnd_una: {}, snd_nxt: {}",
                    String::from_utf8_lossy(head),
                    self.snd_una,
                    self.snd_nxt
                );

                // TODO: schedule the RTO, be ready to retransmit everything starting from snd.una
                self.timers.insert(
                    self.snd_una,
                    TimerEntry {
                        expires_at: Instant::now() + self.rto,
                        snd_nxt: self.snd_nxt,
                        payload_len: head.len(),
                    },
                );

                // When the sender creates a segment and transmits it the sender advances SND.NXT
                self.snd_nxt = self.snd_nxt.wrapping_add(head.len() as u32);
                println!("snd_nxt changed: {}", self.snd_nxt);
            }
        }

        // TODO: eighth, check the FIN bit
        if tcph.fin() {
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

    fn process_syn_sent(
        &mut self,
        dev: &mut device::TunDevice,
        hdr: &etherparse::TcpHeaderSlice,
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
                return self.write_all(
                    dev,
                    self.snd_nxt,
                    Some(self.rcv_nxt),
                    TcpFlags::default(),
                    &[],
                );
            }
        }

        Ok(())
    }

    fn process_close(
        &mut self,
        dev: &mut device::TunDevice,
        hdr: &etherparse::TcpHeaderSlice,
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

    fn is_acceptable(&self, hdr: &etherparse::TcpHeaderSlice, len: usize) -> bool {
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

        let seg_seq = hdr.sequence_number();
        let seg_len = Self::segment_length(hdr, len);
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
        self.write_all(
            dev,
            self.snd_nxt,
            Some(self.rcv_nxt),
            TcpFlags::default(),
            &[],
        )
    }

    fn write_rst(&mut self, dev: &mut device::TunDevice, seq: u32) -> io::Result<()> {
        self.rcv_wnd = 0;
        let flags = TcpFlags {
            rst: true,
            ..Default::default()
        };
        self.write_all(dev, seq, None, flags, &[])
    }

    fn write_rst_ack(
        &mut self,
        dev: &mut device::TunDevice,
        seq: u32,
        seg_len: u32,
    ) -> io::Result<()> {
        // <SEQ=0><ACK=SEG.SEQ+SEG.LEN><CTL=RST,ACK>
        let flags = TcpFlags {
            rst: true,
            ..Default::default()
        };
        self.rcv_wnd = 0;
        self.write_all(dev, 0, Some(seq.wrapping_add(seg_len)), flags, &[])
    }

    fn write_all(
        &self,
        dev: &mut device::TunDevice,
        seq: u32,
        ack: Option<u32>,
        flags: TcpFlags,
        payload: &[u8],
    ) -> io::Result<()> {
        let mut th = etherparse::TcpHeader::new(
            self.listen_addr.port(),
            self.remote_addr.unwrap().port(),
            seq,
            self.rcv_wnd,
        );

        if let Some(ack_num) = ack {
            th.acknowledgment_number = ack_num;
            th.ack = true;
        }

        th.syn = flags.syn;
        th.psh = flags.psh;
        th.rst = flags.rst;

        let builder = etherparse::PacketBuilder::ipv4(
            self.listen_addr.ip().octets(),
            self.remote_addr.unwrap().ip().octets(),
            HOP_LIMIT,
        )
        .tcp_header(th);

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
