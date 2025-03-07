## Gaining hands-on experience with RFC 793 protocol implementation using a TUN device

### Send Sequence Space:

                                   1         2          3          4
                              ----------|----------|----------|----------
                                     SND.UNA    SND.NXT    SND.UNA
                                                          +SND.WND

                        1 - old sequence numbers which have been acknowledged
                        2 - sequence numbers of unacknowledged data
                        3 - sequence numbers allowed for new data transmission
                        4 - future sequence numbers which are not yet allowed

                                          Send Sequence Space

### Receive Sequence Space

                                       1          2          3
                                   ----------|----------|----------
                                          RCV.NXT    RCV.NXT
                                                    +RCV.WND

                        1 - old sequence numbers which have been acknowledged
                        2 - sequence numbers allowed for new reception
                        3 - future sequence numbers which are not yet allowed

                                         Receive Sequence Space

### Notes on RFC793

When the TCP transmits a segment containing data, it puts a copy on a retransmission queue and starts a timer; when the acknowledgment for that data is received, the segment is deleted from the queue. If the acknowledgment is not
received before the timer runs out, the segment is retransmitted.

The TCP does not call on the
network device driver directly, but rather calls on the internet
datagram protocol module which may in turn call on the device driver.

We envision that processes
may "own" ports, and that processes can initiate connections only on
the ports they own. (Means for implementing ownership is a local
issue, but we envision a Request Port user command, or a method of
uniquely allocating a group of ports to a given process, e.g., by
associating the high order bits of a port name with a given process.)

There are two principal cases for matching the sockets in the local
passive OPENs and an foreign active OPENs. In the first case, the
local passive OPENs has fully specified the foreign socket. In this
case, the match must be exact. In the second case, the local passive
OPENs has left the foreign socket unspecified. In this case, any
foreign socket is acceptable as long as the local sockets match.
Other possibilities include partially restricted matches.

When a receiving TCP sees the PUSH flag, it must not wait for more data from
the sending TCP before passing the data to the receiving process.

Among the variables stored in the TCB are the local and remote socket numbers, the security and
precedence of the connection, pointers to the user's send and receive
buffers, pointers to the retransmit queue and to the current segment.
In addition several variables relating to the send and receive
sequence numbers are stored in the TCB.

It is essential to remember that the actual sequence number space is
finite, though very large. This space ranges from 0 to 2**32 - 1.
Since the space is finite, all arithmetic dealing with sequence
numbers must be performed modulo 2**32. This unsigned arithmetic
preserves the relationship of sequence numbers as they cycle from
2\*\*32 - 1 to 0 again.

Note that when the receive window is zero no segments should be
acceptable except ACK segments. Thus, it is be possible for a TCP to
maintain a zero receive window while transmitting data and receiving
ACKs. However, even when the receive window is zero, a TCP must
process the RST and URG fields of all incoming segments.

The CLOSE user call implies a push function, as does the FIN control
flag in an incoming segment.

If more data
arrives than can be accepted, it will be discarded. This will result
in excessive retransmissions, adding unnecessarily to the load on the
network and the TCPs. Indicating a small window may restrict the
transmission of data to the point of introducing a round trip delay
between each new segment transmitted.

psh means that sender is sending all data it has got, i.e a receiver should not wait for
more data;

## Implementing RTO time-out:

Basically, every RTO timeot is associated with an unacknowledged sequence number. If
timeout expires and there is no ack received, the segment is retransmitted starting from
that sequence number.

I don't even fucking know what the fuck I want to achive here. I dont want to write shitty
ccode but i wind up doing that. it's not good.

BTreeMap + HashMap
In this TCP impelmentation per-connection retransmission timer is used.

### Timers

Each TCB maintains a sorted map(BTreeMap) of SND_UNA's transmitted but not yet acknowledged. In
case incoming ACK is grater than a range of SND_UNA's those entries with those seq numbers
are removed from the map. If keys are seq numbers, values are TimerEntry struct instances,
which consists of an "expires_at" time, which is calculated with RTO and a callback, which
has to be invoked in case timer expires. But I can't store cleanly callbacks in a struct which
derives Hash, as those callbacks can't be compared.

The question is does the tx_buffer queue contain from iss up to snd_una or from snd_una up
to snd_nxt
