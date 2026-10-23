//! Browser native-messaging framing.
//!
//! Every message is a 32-bit length in native byte order followed by that
//! many bytes of UTF-8 JSON. Browsers reject messages from the host larger
//! than 1 MB ([`MAX_OUTGOING`]); messages to the host are capped at
//! [`MAX_INCOMING`] here (the browsers allow up to 4 GiB, an extension never
//! needs that much).

use serde_json::Value;
use std::fmt;
use std::io::{self, Read, Write};

/// Largest message the host may send (Chrome and Firefox limit: 1 MB).
pub const MAX_OUTGOING: usize = 1024 * 1024;

/// Largest message the host accepts (64 MiB).
pub const MAX_INCOMING: usize = 64 * 1024 * 1024;

/// Error reading or writing a framed message.
#[derive(Debug)]
pub enum FrameError {
    /// The stream ended inside a length prefix or a message body
    Truncated,
    /// The length prefix exceeds the limit
    TooLarge(usize),
    /// The body is not UTF-8 JSON
    InvalidJson(String),
    /// I/O error
    Io(io::Error),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::Truncated => write!(f, "message truncated"),
            FrameError::TooLarge(n) => write!(f, "message of {n} bytes exceeds the limit"),
            FrameError::InvalidJson(e) => write!(f, "message is not valid JSON: {e}"),
            FrameError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            FrameError::Truncated
        } else {
            FrameError::Io(e)
        }
    }
}

/// Read one message. `Ok(None)` when the stream ends cleanly between
/// messages (the browser closed the port).
pub fn read_message<R: Read>(reader: &mut R, max: usize) -> Result<Option<Value>, FrameError> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < len.len() {
        match reader.read(&mut len[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(FrameError::Truncated),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    let len = u32::from_ne_bytes(len) as usize;
    if len > max {
        return Err(FrameError::TooLarge(len));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| FrameError::InvalidJson(e.to_string()))
}

/// Encode `msg` as one frame, refusing bodies larger than `max`.
pub fn encode_message(msg: &Value, max: usize) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(msg).map_err(|e| FrameError::InvalidJson(e.to_string()))?;
    if body.len() > max {
        return Err(FrameError::TooLarge(body.len()));
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_ne_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// Write one message (at most [`MAX_OUTGOING`] bytes) and flush.
pub fn write_message<W: Write>(writer: &mut W, msg: &Value) -> Result<(), FrameError> {
    let frame = encode_message(msg, MAX_OUTGOING)?;
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn round_trip() {
        let mut buf = Vec::new();
        let a = json!({"type": "ping", "id": 1});
        let b = json!({"type": "download", "url": "https://example.com/é"});
        write_message(&mut buf, &a).unwrap();
        write_message(&mut buf, &b).unwrap();
        let mut r = Cursor::new(buf);
        assert_eq!(read_message(&mut r, MAX_INCOMING).unwrap(), Some(a));
        assert_eq!(read_message(&mut r, MAX_INCOMING).unwrap(), Some(b));
        assert_eq!(read_message(&mut r, MAX_INCOMING).unwrap(), None);
    }

    #[test]
    fn oversize_is_rejected() {
        let big = json!({"s": "x".repeat(MAX_OUTGOING)});
        assert!(matches!(
            write_message(&mut Vec::new(), &big),
            Err(FrameError::TooLarge(_))
        ));
        let mut frame = ((MAX_INCOMING + 1) as u32).to_ne_bytes().to_vec();
        frame.extend_from_slice(b"{}");
        assert!(matches!(
            read_message(&mut Cursor::new(frame), MAX_INCOMING),
            Err(FrameError::TooLarge(_))
        ));
    }

    #[test]
    fn truncated_input() {
        // Inside the length prefix
        assert!(matches!(
            read_message(&mut Cursor::new(vec![5u8, 0]), MAX_INCOMING),
            Err(FrameError::Truncated)
        ));
        // Inside the body
        let mut frame = 10u32.to_ne_bytes().to_vec();
        frame.extend_from_slice(b"{\"a\"");
        assert!(matches!(
            read_message(&mut Cursor::new(frame), MAX_INCOMING),
            Err(FrameError::Truncated)
        ));
        // Not JSON
        let mut frame = 3u32.to_ne_bytes().to_vec();
        frame.extend_from_slice(b"abc");
        assert!(matches!(
            read_message(&mut Cursor::new(frame), MAX_INCOMING),
            Err(FrameError::InvalidJson(_))
        ));
    }
}
