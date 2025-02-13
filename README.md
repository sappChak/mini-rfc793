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

When a
receiving TCP sees the PUSH flag, it must not wait for more data from
the sending TCP before passing the data to the receiving process.

Among the variables stored in the
TCB are the local and remote socket numbers, the security and
precedence of the connection, pointers to the user's send and receive
buffers, pointers to the retransmit queue and to the current segment.
In addition several variables relating to the send and receive
sequence numbers are stored in the TCB.
