//! Native messaging framing: each message is UTF-8 JSON preceded by its length
//! as a 32-bit integer in native byte order.

use std::io::{self, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Browsers refuse host replies above 1 MiB; requests are capped the same way
/// so a broken peer can't make the host allocate gigabytes.
pub const MAX_MESSAGE: usize = 1024 * 1024;

/// Reads one message, or `None` when the browser closed the pipe.
pub fn read_message<T: DeserializeOwned>(reader: &mut impl Read) -> io::Result<Option<T>> {
    let mut len = [0u8; 4];
    match reader.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_ne_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message of {len} bytes exceeds the {MAX_MESSAGE} byte limit"),
        ));
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn write_message(writer: &mut impl Write, message: &impl Serialize) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    if body.len() > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reply exceeds the native messaging limit",
        ));
    }
    writer.write_all(&(body.len() as u32).to_ne_bytes())?;
    writer.write_all(&body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn round_trips_several_messages() {
        let mut wire = Vec::new();
        write_message(&mut wire, &json!({"type": "ping"})).unwrap();
        write_message(&mut wire, &json!({"n": "ü"})).unwrap();
        assert_eq!(&wire[..4], &15u32.to_ne_bytes());

        let mut reader = Cursor::new(wire);
        let first: Value = read_message(&mut reader).unwrap().unwrap();
        let second: Value = read_message(&mut reader).unwrap().unwrap();
        assert_eq!(first, json!({"type": "ping"}));
        assert_eq!(second, json!({"n": "ü"}));
        assert!(read_message::<Value>(&mut reader).unwrap().is_none());
    }

    #[test]
    fn rejects_oversized_and_truncated_messages() {
        let huge = (MAX_MESSAGE as u32 + 1).to_ne_bytes();
        let err = read_message::<Value>(&mut Cursor::new(huge)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        let mut truncated = 10u32.to_ne_bytes().to_vec();
        truncated.extend_from_slice(b"{\"a\"");
        let err = read_message::<Value>(&mut Cursor::new(truncated)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);

        let mut garbage = 3u32.to_ne_bytes().to_vec();
        garbage.extend_from_slice(b"{{{");
        let err = read_message::<Value>(&mut Cursor::new(garbage)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
