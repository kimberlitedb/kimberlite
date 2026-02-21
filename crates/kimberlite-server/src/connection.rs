//! Connection state management.

use std::io::{self, Read, Write};
use std::time::Instant;

use bytes::BytesMut;
use mio::net::TcpStream;
use mio::{Interest, Token};

use kimberlite_wire::{FRAME_HEADER_SIZE, Frame, Request, Response};

use crate::config::RateLimitConfig;
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
    /// Last activity timestamp for idle timeout tracking.
    pub last_activity: Instant,
    /// Rate limiting state.
    pub rate_limiter: Option<RateLimiter>,
    /// Tenant priority tag for QoS-based rate limiting.
    pub tenant_priority: Option<crate::config::TenantPriority>,
    /// Authenticated identity for this connection (set after successful Handshake).
    pub authenticated_identity: Option<crate::auth::AuthenticatedIdentity>,
}

/// O(1) token bucket rate limiter.
///
/// Uses a token bucket algorithm instead of tracking individual request timestamps.
/// At `max_requests` requests per `window`, tokens are added at a rate of
/// `max_requests / window` per second. Each request consumes one token.
///
/// This is O(1) per check (vs O(n) for sliding window with `retain()`).
pub struct RateLimiter {
    /// Maximum tokens (bucket capacity).
    capacity: f64,
    /// Current token count.
    tokens: f64,
    /// Tokens added per nanosecond.
    refill_rate_per_ns: f64,
    /// Last time tokens were refilled.
    last_refill: Instant,
}

impl RateLimiter {
    /// Creates a new rate limiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        let capacity = f64::from(config.max_requests);
        let window_ns = config.window.as_nanos() as f64;
        Self {
            capacity,
            tokens: capacity, // Start full
            refill_rate_per_ns: capacity / window_ns,
            last_refill: Instant::now(),
        }
    }

    /// Checks if a request should be allowed.
    ///
    /// Returns `true` if the request is allowed, `false` if rate limited.
    /// O(1) time complexity.
    pub fn check(&mut self) -> bool {
        self.refill();

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Refills tokens based on elapsed time since last refill.
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed_ns = now.duration_since(self.last_refill).as_nanos() as f64;
        self.tokens = (self.tokens + elapsed_ns * self.refill_rate_per_ns).min(self.capacity);
        self.last_refill = now;
    }

    /// Returns the approximate number of available tokens.
    #[allow(dead_code)]
    pub fn current_count(&self) -> usize {
        // Approximate: doesn't account for time since last refill.
        // Tokens are always >= 0.0 by construction (check subtracts, refill caps at capacity).
        #[allow(clippy::cast_sign_loss)]
        let count = self.tokens as usize;
        count
    }
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
            last_activity: Instant::now(),
            rate_limiter: None,
            tenant_priority: None,
            authenticated_identity: None,
        }
    }

    /// Creates a new connection with rate limiting.
    pub fn with_rate_limit(
        token: Token,
        stream: TcpStream,
        buffer_size: usize,
        rate_config: RateLimitConfig,
    ) -> Self {
        Self {
            token,
            stream,
            read_buf: BytesMut::with_capacity(buffer_size),
            write_buf: BytesMut::with_capacity(buffer_size),
            closing: false,
            last_activity: Instant::now(),
            rate_limiter: Some(RateLimiter::new(rate_config)),
            tenant_priority: None,
            authenticated_identity: None,
        }
    }

    /// Sets the tenant priority for tag-based QoS rate limiting.
    ///
    /// Replaces the connection's rate limiter with one appropriate for
    /// the tenant's priority tier.
    pub fn set_tenant_rate_limit(
        &mut self,
        priority: crate::config::TenantPriority,
        config: Option<RateLimitConfig>,
    ) {
        self.tenant_priority = Some(priority);
        self.rate_limiter = config.map(RateLimiter::new);
    }

    /// Updates the last activity timestamp.
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Checks if the connection has been idle for longer than the timeout.
    pub fn is_idle(&self, timeout: std::time::Duration) -> bool {
        self.last_activity.elapsed() > timeout
    }

    /// Checks if a request should be rate limited.
    ///
    /// Returns `true` if the request is allowed, `false` if rate limited.
    pub fn check_rate_limit(&mut self) -> bool {
        match &mut self.rate_limiter {
            Some(limiter) => limiter.check(),
            None => true, // No rate limiting configured
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
