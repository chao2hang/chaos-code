//! SocketProtocol framing: the 13-byte envelope between the raw WebSocket
//! bytes and the serialized RPC chunks.
//!
//! Mirrors `packages/rpc/src/protocol.ts`: `[type:u8=1(Regular)][id:u32BE]
//! [ack:u32BE][len:u32BE][payload]`, no CRC (the simplified ZCode transport
//! omits it), and a chunk stream that tolerates payloads split across
//! arbitrary socket reads.

/// Size of the framing header: 1 byte type + 4 bytes id + 4 bytes ack + 4 bytes length.
pub const HEADER_SIZE: usize = 13;
/// Message type carried on the wire for every RPC message (`ProtocolMessageType.Regular`).
pub const MESSAGE_TYPE_REGULAR: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FramingError {
    #[error("frame header declares {declared} payload bytes but only {available} are present")]
    TruncatedFrame { declared: usize, available: usize },
    #[error("frame type {0} is not Regular(1); this transport carries only Regular messages")]
    UnexpectedMessageType(u8),
}

/// Wrap one serialized RPC message (header chunk + body chunk) in the
/// 13-byte transport envelope.
pub fn write_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_SIZE + payload.len());
    out.push(MESSAGE_TYPE_REGULAR);
    out.extend_from_slice(&0u32.to_be_bytes()); // id
    out.extend_from_slice(&0u32.to_be_bytes()); // ack
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// A message pulled out of the chunk stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub message_type: u8,
    pub id: u32,
    pub ack: u32,
    pub payload: Vec<u8>,
}

/// Incremental chunk stream: feed socket bytes in any chunking, pull complete
/// frames out. Non-Regular control frames (ack/keepalive/…) are consumed and
/// skipped, matching upstream `SocketProtocol.readMessages`, which only fires
/// `onMessage` for `Regular`.
#[derive(Debug, Default)]
pub struct ChunkStream {
    buf: Vec<u8>,
}

impl ChunkStream {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed raw socket bytes; returns every complete frame they completed.
    pub fn accept(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, FramingError> {
        self.buf.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            if self.buf.len() < HEADER_SIZE {
                break;
            }
            let message_type = self.buf[0];
            let id = u32::from_be_bytes(self.buf[1..5].try_into().expect("4 bytes"));
            let ack = u32::from_be_bytes(self.buf[5..9].try_into().expect("4 bytes"));
            let declared =
                u32::from_be_bytes(self.buf[9..13].try_into().expect("4 bytes")) as usize;
            if self.buf.len() < HEADER_SIZE + declared {
                break;
            }
            let payload = self.buf[HEADER_SIZE..HEADER_SIZE + declared].to_vec();
            self.buf.drain(..HEADER_SIZE + declared);
            if message_type != MESSAGE_TYPE_REGULAR {
                continue; // consumed and skipped, like upstream
            }
            frames.push(Frame {
                message_type,
                id,
                ack,
                payload,
            });
        }
        Ok(frames)
    }

    pub fn buffered_bytes(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_layout_matches_upstream() {
        let frame = write_frame(&[0xaa, 0xbb, 0xcc]);
        assert_eq!(frame.len(), HEADER_SIZE + 3);
        assert_eq!(frame[0], 1);
        assert_eq!(&frame[1..5], &[0, 0, 0, 0]);
        assert_eq!(&frame[5..9], &[0, 0, 0, 0]);
        assert_eq!(&frame[9..13], &[0, 0, 0, 3]);
        assert_eq!(&frame[13..], &[0xaa, 0xbb, 0xcc]);
    }

    #[test]
    fn splits_and_reassembles_arbitrary_chunk_boundaries() {
        let a = write_frame(&[1, 2, 3, 4, 5]);
        let b = write_frame(&[9; 40]);
        let mut wire = Vec::new();
        wire.extend_from_slice(&a);
        wire.extend_from_slice(&b);
        let boundaries = [a.len(), wire.len()];

        // Feed one byte at a time: a frame may only complete on the last byte
        // of its payload.
        let mut stream = ChunkStream::new();
        let mut total = 0usize;
        for (index, byte) in wire.iter().enumerate() {
            let completed = stream.accept(&[*byte]).unwrap();
            if boundaries.contains(&(index + 1)) {
                assert_eq!(completed.len(), 1, "frame completes at its payload end");
                total += completed.len();
            } else {
                assert!(completed.is_empty(), "no frame can complete mid-payload");
            }
        }
        assert_eq!(total, 2);
        assert_eq!(stream.buffered_bytes(), 0);

        // Feed everything at once: both frames, payloads intact.
        let mut stream = ChunkStream::new();
        let frames = stream.accept(&wire).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].payload, vec![1, 2, 3, 4, 5]);
        assert_eq!(frames[1].payload, vec![9; 40]);
        assert_eq!(stream.buffered_bytes(), 0);
    }

    #[test]
    fn control_frames_are_skipped_like_upstream() {
        let mut control = Vec::new();
        control.push(3); // Ack type
        control.extend_from_slice(&0u32.to_be_bytes());
        control.extend_from_slice(&0u32.to_be_bytes());
        control.extend_from_slice(&2u32.to_be_bytes());
        control.extend_from_slice(&[0xee, 0xff]);
        let regular = write_frame(&[0x01]);
        let mut wire = control;
        wire.extend_from_slice(&regular);

        let mut stream = ChunkStream::new();
        let frames = stream.accept(&wire).unwrap();
        assert_eq!(frames.len(), 1, "only the Regular frame is delivered");
        assert_eq!(frames[0].payload, vec![0x01]);
    }
}
