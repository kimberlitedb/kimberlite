//! Connection state management.

use std::io::{self, Read, Write};

use bytes::BytesMut;
use mio::net::TcpStream;
use mio::{Interest, Token};

use vdb_wire::{FRAME_HEADER_SIZE, Frame, Request, Response};

use crate::error::ServerResult;

/// State of a client connection.
pub struct Connection {
    /// Unique token for this connection (kept for debugging).
    #[allow(dead_code)]
    pub token: Token,
    /// TCP stream.
    pub stream: TcpStream,
    /// Read buffer.
    pub read_buf: BytesMut,
    /// Write buffer.
    pub write_buf: BytesMut,
    /// Whether the connection is closing.
    pub closing: bool,
}

impl Connection {
    /// Creates a new connection.
    pub fn new(token: Token, stream: TcpStream, buffer_size: usize) -> Self {
        Self {
            token,
            stream,
            read_buf: BytesMut::with_capacity(buffer_size),
            write_buf: BytesMut::with_capacity(buffer_size),
            closing: false,
        }
    }

    /// Reads data from the socket into the read buffer.
    ///
    /// Returns `true` if the connection is still open.
    pub fn read(&mut self) -> io::Result<bool> {
        // Use a temporary stack buffer to avoid unsafe
        let mut temp_buf = [0u8; 4096];

        loop {
            match self.stream.read(&mut temp_buf) {
                Ok(0) => {
                    // Connection closed
                    return Ok(false);
                }
                Ok(n) => {
                    self.read_buf.extend_from_slice(&temp_buf[..n]);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // No more data available
                    return Ok(true);
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Writes data from the write buffer to the socket.
    ///
    /// Returns `true` if all data was written.
    pub fn write(&mut self) -> io::Result<bool> {
        while !self.write_buf.is_empty() {
            match self.stream.write(&self.write_buf) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "failed to write to socket",
                    ));
                }
                Ok(n) => {
                    let _ = self.write_buf.split_to(n);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // Socket not ready for writing
                    return Ok(false);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(true)
    }

    /// Attempts to decode a request from the read buffer.
    pub fn try_decode_request(&mut self) -> ServerResult<Option<Request>> {
        // Try to decode a frame
        let frame = Frame::decode(&mut self.read_buf)?;

        match frame {
            Some(f) => {
                // Decode the request from the frame
                let request = Request::from_frame(&f)?;
                Ok(Some(request))
            }
            None => Ok(None),
        }
    }

    /// Queues a response to be sent.
    pub fn queue_response(&mut self, response: &Response) -> ServerResult<()> {
        let frame = response.to_frame()?;
        frame.encode(&mut self.write_buf);
        Ok(())
    }

    /// Returns the interest flags for this connection.
    pub fn interest(&self) -> Interest {
        if self.write_buf.is_empty() {
            Interest::READABLE
        } else {
            Interest::READABLE | Interest::WRITABLE
        }
    }

    /// Returns true if there's pending data to process.
    pub fn has_pending_data(&self) -> bool {
        self.read_buf.len() >= FRAME_HEADER_SIZE
    }
}
