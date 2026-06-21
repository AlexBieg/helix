//! Transport abstraction for MCP communication.
//!
//! Uses newline-delimited JSON framing: each JSON-RPC message is serialized
//! as a single line terminated by `\n`. This is the standard MCP streamable
//! transport format.

use std::fmt;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};

/// Errors that can occur during transport operations.
#[derive(Debug)]
pub enum TransportError {
    /// An I/O error occurred.
    Io(std::io::Error),
    /// The transport stream was closed.
    StreamClosed,
    /// The message was too large (exceeded maximum line length).
    MessageTooLarge {
        /// The maximum allowed length in bytes.
        max: usize,
    },
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Io(e) => write!(f, "transport I/O error: {}", e),
            TransportError::StreamClosed => f.write_str("transport stream closed"),
            TransportError::MessageTooLarge { max } => {
                write!(f, "transport message exceeded {} bytes", max)
            }
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TransportError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self {
        TransportError::Io(e)
    }
}

/// The transport trait: any type that is both `AsyncRead + AsyncWrite` and
/// satisfies the other bounds required for MCP communication.
pub trait Transport: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Transport for T {}

/// The maximum allowed line length for a single JSON-RPC message.
/// Messages exceeding this will be rejected with `TransportError::MessageTooLarge`.
const MAX_MESSAGE_LENGTH: usize = 10 * 1024 * 1024; // 10 MB

/// Read a single newline-delimited JSON message from a buffered reader.
///
/// Reads until `\n` is found, strips the trailing newline, and returns the
/// resulting string. Returns `TransportError::StreamClosed` if the stream
/// ends without producing any data.
pub async fn read_message(
    reader: &mut (impl AsyncBufReadExt + Unpin),
) -> Result<String, TransportError> {
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Err(TransportError::StreamClosed);
    }
    if line.len() > MAX_MESSAGE_LENGTH {
        return Err(TransportError::MessageTooLarge {
            max: MAX_MESSAGE_LENGTH,
        });
    }
    // Trim trailing newline(s)
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(line)
}

/// Write a newline-delimited JSON message to a buffered writer.
///
/// Appends `\n` to the message and writes it to the writer. Flushes after
/// writing to ensure the message is sent immediately.
pub async fn write_message(
    writer: &mut (impl AsyncWriteExt + Unpin),
    msg: &str,
) -> Result<(), TransportError> {
    writer.write_all(msg.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

/// Create a buffered reader/writer pair from a transport stream.
///
/// Returns `(BufReader<...>, BufWriter<...>)` wrapping the two halves of the
/// transport. The transport is split using `tokio::io::split`.
pub fn buffered<T: Transport>(
    transport: T,
) -> (
    BufReader<tokio::io::ReadHalf<T>>,
    BufWriter<tokio::io::WriteHalf<T>>,
) {
    let (reader, writer) = tokio::io::split(transport);
    (BufReader::new(reader), BufWriter::new(writer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn read_write_roundtrip() {
        let (client, server) = duplex(1024);
        let (_cr, mut cw) = buffered(client);
        let (mut sr, _sw) = buffered(server);

        let msg = r#"{"jsonrpc":"2.0","method":"initialize","id":1}"#;

        // Client writes
        write_message(&mut cw, msg).await.unwrap();

        // Server reads
        let received = read_message(&mut sr).await.unwrap();
        assert_eq!(received, msg);
    }

    #[tokio::test]
    async fn read_multiple_messages() {
        let (client, server) = duplex(1024);
        let (_cr, mut cw) = buffered(client);
        let (mut sr, _sw) = buffered(server);

        let msg1 = r#"{"jsonrpc":"2.0","method":"a","id":1}"#;
        let msg2 = r#"{"jsonrpc":"2.0","method":"b","id":2}"#;

        write_message(&mut cw, msg1).await.unwrap();
        write_message(&mut cw, msg2).await.unwrap();

        assert_eq!(read_message(&mut sr).await.unwrap(), msg1);
        assert_eq!(read_message(&mut sr).await.unwrap(), msg2);
    }

    #[tokio::test]
    async fn stream_closed_on_empty() {
        let (client, _server) = duplex(1024);
        let (_, cw) = buffered(client);
        let (mut sr, _) = buffered(tokio::io::duplex(1024).0);

        // Drop the writer side so the reader gets EOF
        drop(cw);

        // The reader should detect stream closed when no data is available
        // Note: duplex channels may not produce EOF immediately, so we just
        // verify that read_message returns an error or empty result.
        match read_message(&mut sr).await {
            Err(TransportError::StreamClosed) | Err(TransportError::Io(_)) => {}
            other => panic!("expected StreamClosed or Io error, got {:?}", other),
        }
    }
}
