//! Native Messaging framing: a 4-byte length in native byte order, then UTF-8 JSON.
//! The same framing is used on the named pipe between the host and memwarden.

use std::io::{self, Read, Write};

/// Largest message accepted in either direction. Chrome itself allows 1 MB
/// from the host; protocol messages are far smaller, so anything bigger is a bug.
pub const MAX_FRAME: usize = 64 * 1024;

/// Reads one message. `Ok(None)` means the stream ended cleanly between messages.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < len.len() {
        match r.read(&mut len[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let len = u32::from_ne_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(too_large(len));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Writes one message and flushes it, so the other side sees it at once.
pub fn write_frame(w: &mut impl Write, body: &[u8]) -> io::Result<()> {
    if body.len() > MAX_FRAME {
        return Err(too_large(body.len()));
    }
    w.write_all(&(body.len() as u32).to_ne_bytes())?;
    w.write_all(body)?;
    w.flush()
}

fn too_large(len: usize) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("message of {len} bytes exceeds {MAX_FRAME}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, br#"{"v":1}"#).unwrap();
        write_frame(&mut buf, b"").unwrap();
        assert_eq!(&buf[..4], &7u32.to_ne_bytes());

        let mut r = buf.as_slice();
        assert_eq!(read_frame(&mut r).unwrap().unwrap(), br#"{"v":1}"#);
        assert_eq!(read_frame(&mut r).unwrap().unwrap(), b"");
        assert!(read_frame(&mut r).unwrap().is_none());
    }

    #[test]
    fn truncated_stream_is_an_error() {
        let mut r: &[u8] = &[7, 0];
        assert_eq!(read_frame(&mut r).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);

        let mut buf = Vec::new();
        write_frame(&mut buf, b"hello").unwrap();
        let mut r = &buf[..buf.len() - 1];
        assert_eq!(read_frame(&mut r).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn oversized_messages_are_rejected() {
        let mut r: &[u8] = &(MAX_FRAME as u32 + 1).to_ne_bytes();
        assert_eq!(read_frame(&mut r).unwrap_err().kind(), io::ErrorKind::InvalidData);

        let mut buf = Vec::new();
        let big = vec![b' '; MAX_FRAME + 1];
        assert_eq!(write_frame(&mut buf, &big).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(buf.is_empty());
        write_frame(&mut buf, &big[..MAX_FRAME]).unwrap();
    }
}
