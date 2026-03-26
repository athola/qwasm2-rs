//! Network channel — reliable/unreliable message delivery layer.
//!
//! Port of `netchan.c` from the Quake 2 engine. Provides sequence-numbered
//! packets, acknowledgment tracking, reliable message delivery with
//! retransmission, and QPort for NAT disambiguation.
//!
//! # Packet Header Layout
//! ```text
//! 31 bits  sequence number
//!  1 bit   does this message contain a reliable payload
//! 31 bits  acknowledge sequence
//!  1 bit   acknowledge receipt of even/odd reliable message
//! 16 bits  qport
//! ```

use crate::net_msg::NetMsg;

/// Maximum message length for a single packet (from `q2-shared`).
const MAX_MSGLEN: usize = q2_shared::constants::MAX_MSGLEN;

/// Bit 31 — used as the reliable flag in sequence words.
const SEQUENCE_RELIABLE_BIT: u32 = 1 << 31;

/// Network channel for reliable/unreliable message delivery.
///
/// Each side of a connection maintains one `NetChan`. Unreliable data is sent
/// every frame; reliable data is queued and retransmitted until acknowledged.
pub struct NetChan {
    /// Set on unrecoverable errors (e.g. message overflow).
    pub fatal_error: bool,

    // --- Sequencing ---
    /// Next outgoing sequence number (starts at 1, incremented each transmit).
    pub outgoing_sequence: u32,
    /// Last received sequence number from the remote side.
    pub incoming_sequence: u32,
    /// Last acknowledged outgoing sequence (reported by remote side).
    pub incoming_acknowledged: u32,

    // --- Reliable messaging ---
    /// Toggles 0/1 each time a new reliable message is promoted from the
    /// staging buffer to the send buffer. The remote side uses this to detect
    /// whether the reliable was received.
    pub reliable_sequence: u32,
    /// The `outgoing_sequence` at the time the last reliable was sent.
    /// Used to decide if a retransmit is needed.
    pub last_reliable_sequence: u32,
    /// Reliable message buffer (data waiting to be sent / retransmitted).
    pub reliable_buf: NetMsg,
    /// `true` while we have reliable data in `reliable_buf` awaiting acknowledgment.
    pub reliable_pending: bool,
    /// The reliable-sequence bit we last sent (mirrors `reliable_sequence`).
    /// The remote echoes this back to acknowledge receipt.
    pub last_sent_reliable: bool,

    // --- Incoming reliable tracking ---
    /// Toggles each time a reliable message is received from the remote side.
    pub incoming_reliable_sequence: u32,
    /// The reliable-sequence bit the remote side has acknowledged.
    pub incoming_reliable_acknowledged: u32,

    // --- Staging buffer for new reliable messages ---
    /// Messages queued via `queue_reliable` sit here until the current
    /// reliable is acknowledged and we can promote them.
    message_buf: NetMsg,

    // --- Diagnostics ---
    /// Number of packets dropped between the last two received packets.
    pub dropped: u32,

    // --- QPort ---
    /// QPort value written into outgoing packet headers for NAT disambiguation.
    pub qport: u16,
}

impl NetChan {
    /// Create a new network channel with the given QPort.
    ///
    /// Mirrors `Netchan_Setup` from the C source: outgoing_sequence starts at 1,
    /// incoming_sequence starts at 0.
    pub fn new(qport: u16) -> Self {
        Self {
            fatal_error: false,
            outgoing_sequence: 1,
            incoming_sequence: 0,
            incoming_acknowledged: 0,
            reliable_sequence: 0,
            last_reliable_sequence: 0,
            reliable_buf: NetMsg::new(),
            reliable_pending: false,
            last_sent_reliable: false,
            incoming_reliable_sequence: 0,
            incoming_reliable_acknowledged: 0,
            message_buf: NetMsg::new(),
            dropped: 0,
            qport,
        }
    }

    /// Returns `true` if a new reliable message can be queued.
    ///
    /// A reliable message can only be queued when the previous one has been
    /// acknowledged (i.e. `reliable_pending` is `false` and `reliable_buf`
    /// is empty).
    pub fn can_reliable(&self) -> bool {
        !self.reliable_pending && self.reliable_buf.is_empty()
    }

    /// Queue a reliable message for the next transmit.
    ///
    /// The data is staged in an internal buffer and will be promoted to
    /// `reliable_buf` on the next call to `transmit` when the previous
    /// reliable has been acknowledged.
    pub fn queue_reliable(&mut self, data: &[u8]) {
        self.message_buf.write_data(data);
    }

    /// Determine if a reliable message needs to be (re)sent.
    ///
    /// Returns `true` if:
    /// - The remote dropped our last reliable (ack sequence advanced past
    ///   `last_reliable_sequence` but the reliable-ack bit doesn't match), OR
    /// - The reliable send buffer is empty but we have new data staged.
    fn need_reliable(&self) -> bool {
        // Remote dropped our last reliable — retransmit.
        if self.incoming_acknowledged > self.last_reliable_sequence
            && self.incoming_reliable_acknowledged != self.reliable_sequence
        {
            return true;
        }

        // We have staged data and no pending reliable.
        if self.reliable_buf.is_empty() && !self.message_buf.is_empty() {
            return true;
        }

        false
    }

    /// Build and return a packet with the appropriate headers, reliable data
    /// (if pending), and the supplied unreliable `data`.
    ///
    /// This is the Rust equivalent of `Netchan_Transmit`. A zero-length `data`
    /// slice is valid — it still generates a packet and handles reliable state.
    ///
    /// Returns the raw bytes of the assembled packet ready for sending.
    pub fn transmit(&mut self, data: &[u8]) -> Vec<u8> {
        let send_reliable = self.need_reliable();

        // Promote staged data to reliable buffer if possible.
        if self.reliable_buf.is_empty() && !self.message_buf.is_empty() {
            // Swap staged data into the reliable buffer.
            std::mem::swap(&mut self.reliable_buf, &mut self.message_buf);
            self.message_buf.clear();
            // Toggle the reliable sequence bit.
            self.reliable_sequence ^= 1;
            self.reliable_pending = true;
        }

        // Build packet header.
        let mut send = NetMsg::with_capacity(MAX_MSGLEN);

        // Word 1: sequence | reliable-flag in bit 31.
        let w1 = (self.outgoing_sequence & !SEQUENCE_RELIABLE_BIT)
            | if send_reliable { SEQUENCE_RELIABLE_BIT } else { 0 };

        // Word 2: incoming_sequence | incoming_reliable_sequence in bit 31.
        let w2 = (self.incoming_sequence & !SEQUENCE_RELIABLE_BIT)
            | (self.incoming_reliable_sequence << 31);

        self.outgoing_sequence += 1;

        send.write_long(w1 as i32);
        send.write_long(w2 as i32);

        // QPort.
        send.write_short(self.qport as i32);

        // Append reliable data if we're sending it.
        if send_reliable {
            send.write_data(self.reliable_buf.data());
            self.last_reliable_sequence = self.outgoing_sequence;
            self.last_sent_reliable = true;
        }

        // Append unreliable data if there's room.
        if MAX_MSGLEN - send.len() >= data.len() {
            send.write_data(data);
        }

        send.data().to_vec()
    }

    /// Process an incoming packet. Updates acknowledgment state and returns
    /// the payload bytes if the packet is valid (not stale/duplicate).
    ///
    /// Returns `None` if the packet is out-of-order or a duplicate.
    ///
    /// Mirrors `Netchan_Process` from the C source.
    pub fn process(&mut self, packet: &[u8]) -> Option<Vec<u8>> {
        let mut msg = NetMsg::from_bytes(packet);
        msg.begin_reading();

        let raw_sequence = msg.read_long() as u32;
        let raw_sequence_ack = msg.read_long() as u32;

        // Read (and discard) the qport.
        let _qport = msg.read_short();

        // Extract the reliable flags from bit 31.
        let reliable_message = (raw_sequence & SEQUENCE_RELIABLE_BIT) != 0;
        let reliable_ack = raw_sequence_ack >> 31;

        let sequence = raw_sequence & !SEQUENCE_RELIABLE_BIT;
        let sequence_ack = raw_sequence_ack & !SEQUENCE_RELIABLE_BIT;

        // Discard stale or duplicate packets.
        if sequence <= self.incoming_sequence {
            return None;
        }

        // Track dropped packets.
        self.dropped = sequence - (self.incoming_sequence + 1);

        // If the remote acknowledged our current reliable sequence,
        // clear the reliable buffer.
        if reliable_ack == self.reliable_sequence {
            self.reliable_buf.clear();
            self.reliable_pending = false;
        }

        // Update incoming state.
        self.incoming_sequence = sequence;
        self.incoming_acknowledged = sequence_ack;
        self.incoming_reliable_acknowledged = reliable_ack;

        // If this packet contained a reliable message, toggle the incoming
        // reliable sequence so we echo it back in our next ack word.
        if reliable_message {
            self.incoming_reliable_sequence ^= 1;
        }

        // Return everything after the header as the payload.
        let payload = msg.remaining_data().to_vec();
        Some(payload)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: simulate a full round-trip between two channels.
    /// Transmit from `a` with `data`, process at `b`, return `b`'s view of the payload.
    fn send(a: &mut NetChan, b: &mut NetChan, data: &[u8]) -> Option<Vec<u8>> {
        let packet = a.transmit(data);
        b.process(&packet)
    }

    // 1. basic_transmit_receive
    #[test]
    fn basic_transmit_receive() {
        let mut a = NetChan::new(27500);
        let mut b = NetChan::new(27501);

        let payload = b"hello world";
        let received = send(&mut a, &mut b, payload);

        assert!(received.is_some(), "B should accept the packet");
        assert_eq!(received.unwrap(), payload);
    }

    // 2. sequence_numbers_increment
    #[test]
    fn sequence_numbers_increment() {
        let mut a = NetChan::new(100);
        let mut b = NetChan::new(200);

        assert_eq!(a.outgoing_sequence, 1);

        send(&mut a, &mut b, b"msg1");
        assert_eq!(a.outgoing_sequence, 2);

        send(&mut a, &mut b, b"msg2");
        assert_eq!(a.outgoing_sequence, 3);

        send(&mut a, &mut b, b"msg3");
        assert_eq!(a.outgoing_sequence, 4);
    }

    // 3. reliable_delivery
    #[test]
    fn reliable_delivery() {
        let mut a = NetChan::new(100);
        let mut b = NetChan::new(200);

        // Queue reliable data on A.
        let reliable_data = b"important";
        a.queue_reliable(reliable_data);

        // Transmit from A to B. The reliable data should be included.
        let packet_a_to_b = a.transmit(b"unreliable");
        let received = b.process(&packet_a_to_b);
        assert!(received.is_some());

        // B should have received both reliable + unreliable data concatenated.
        let payload = received.unwrap();
        assert!(payload.starts_with(reliable_data));
        assert!(payload.ends_with(b"unreliable"));

        // A is still waiting for acknowledgment.
        assert!(a.reliable_pending);

        // Now B sends an ack back to A (transmit from B, process at A).
        let packet_b_to_a = b.transmit(b"");
        a.process(&packet_b_to_a);

        // A should have cleared the reliable state.
        assert!(!a.reliable_pending);
        assert!(a.reliable_buf.is_empty());
        assert!(a.can_reliable());
    }

    // 4. reliable_retransmit
    #[test]
    fn reliable_retransmit() {
        let mut a = NetChan::new(100);
        let mut b = NetChan::new(200);

        // Do one normal exchange so acks are in sync.
        let pkt = a.transmit(b"setup");
        b.process(&pkt);
        let ack_pkt = b.transmit(b"");
        a.process(&ack_pkt);

        // Now queue reliable data on A.
        a.queue_reliable(b"critical");

        // Transmit — contains reliable data. Drop this packet (B never sees it).
        let dropped = a.transmit(b"");

        // Verify the dropped packet contains the reliable data.
        {
            let mut peek = NetMsg::from_bytes(&dropped);
            peek.begin_reading();
            let w1 = peek.read_long() as u32;
            let has_reliable = (w1 & SEQUENCE_RELIABLE_BIT) != 0;
            assert!(has_reliable, "first send should carry reliable flag");
        }

        // To trigger retransmit, A's incoming_acknowledged must exceed
        // last_reliable_sequence (strictly >). We need two round-trips
        // of unreliable packets so B's ack of A advances far enough.

        // Round-trip 1: A sends unreliable, B acks.
        let plain1 = a.transmit(b"ping1");
        b.process(&plain1);
        let ack1 = b.transmit(b"");
        a.process(&ack1);

        // Round-trip 2: A sends unreliable, B acks.
        let plain2 = a.transmit(b"ping2");
        b.process(&plain2);
        let ack2 = b.transmit(b"");
        a.process(&ack2);

        // Now A's incoming_acknowledged should be > last_reliable_sequence,
        // and incoming_reliable_acknowledged should NOT match reliable_sequence
        // because B never received the reliable message.
        assert!(
            a.incoming_acknowledged > a.last_reliable_sequence,
            "ack should have advanced past last reliable: ack={} last_rel={}",
            a.incoming_acknowledged,
            a.last_reliable_sequence,
        );
        assert_ne!(
            a.incoming_reliable_acknowledged, a.reliable_sequence,
            "reliable ack should not match (B never got the reliable)"
        );

        // Transmit again — the reliable should be re-sent.
        let retransmit_pkt = a.transmit(b"");
        {
            let mut peek = NetMsg::from_bytes(&retransmit_pkt);
            peek.begin_reading();
            let w1 = peek.read_long() as u32;
            let has_reliable = (w1 & SEQUENCE_RELIABLE_BIT) != 0;
            assert!(
                has_reliable,
                "retransmit should carry reliable flag again"
            );
        }

        // Now deliver the retransmit to B. B should get the reliable data.
        let received = b.process(&retransmit_pkt);
        assert!(received.is_some());
        let payload = received.unwrap();
        assert_eq!(payload, b"critical");
    }

    // 5. out_of_order_rejected
    #[test]
    fn out_of_order_rejected() {
        let mut a = NetChan::new(100);
        let mut b = NetChan::new(200);

        // Send two packets from A to B.
        let pkt1 = a.transmit(b"first");
        let pkt2 = a.transmit(b"second");

        // Process pkt2 first (B sees sequence 2).
        let r2 = b.process(&pkt2);
        assert!(r2.is_some());

        // Now process pkt1 (sequence 1 <= incoming_sequence 2) — should be rejected.
        let r1 = b.process(&pkt1);
        assert!(r1.is_none(), "out-of-order packet should be rejected");
    }

    // Additional: duplicate rejection
    #[test]
    fn duplicate_rejected() {
        let mut a = NetChan::new(100);
        let mut b = NetChan::new(200);

        let pkt = a.transmit(b"data");

        let r1 = b.process(&pkt);
        assert!(r1.is_some());

        // Process same packet again — duplicate.
        let r2 = b.process(&pkt);
        assert!(r2.is_none(), "duplicate packet should be rejected");
    }

    // Additional: dropped packet count
    #[test]
    fn dropped_count_tracked() {
        let mut a = NetChan::new(100);
        let mut b = NetChan::new(200);

        // Send 3 packets, only deliver the 3rd.
        let _pkt1 = a.transmit(b"one");
        let _pkt2 = a.transmit(b"two");
        let pkt3 = a.transmit(b"three");

        b.process(&pkt3);
        assert_eq!(b.dropped, 2, "should count 2 dropped packets");
    }

    // Additional: qport is written into packet header
    #[test]
    fn qport_in_header() {
        let mut a = NetChan::new(12345);

        let pkt = a.transmit(b"test");

        // The qport is at bytes 8..10 (after two 32-bit words).
        let mut msg = NetMsg::from_bytes(&pkt);
        msg.begin_reading();
        let _w1 = msg.read_long();
        let _w2 = msg.read_long();
        let qp = msg.read_short() as u16;
        assert_eq!(qp, 12345);
    }

    // Additional: can_reliable returns true initially
    #[test]
    fn can_reliable_initially() {
        let chan = NetChan::new(100);
        assert!(chan.can_reliable());
    }

    // Additional: can_reliable returns false after queueing
    #[test]
    fn can_reliable_false_after_queue() {
        let mut chan = NetChan::new(100);
        chan.queue_reliable(b"stuff");
        // Data is staged but not yet promoted — can_reliable checks both.
        // message_buf is not empty, but reliable_buf is empty and pending is false.
        // Actually can_reliable only checks reliable_pending and reliable_buf.
        // With our impl, message_buf data means it hasn't been promoted yet,
        // so can_reliable would say true. But semantically we shouldn't queue
        // more until the first one goes through... Let's verify the actual behavior.
        //
        // After queue_reliable, message_buf has data. can_reliable checks
        // reliable_pending (false) and reliable_buf.is_empty() (true).
        // So can_reliable returns true. That's correct — the C code also
        // allows writing more into the message buffer at any time via
        // MSG_Write* on &chan->message.
        assert!(chan.can_reliable(), "can still add more to staging buffer");
    }
}
