//! Sans-I/O wire framing for the Kimberlite RPC protocol.
//!
//! AUDIT-2026-04 S2.1 — split out of [`crate::client`] so the sync
//! [`crate::Client`] and the async [`crate::AsyncClient`] share the
//! same encode / decode primitives. No socket types here — pure
//! `Bytes` ⇄ `Frame` ⇄ `Message` transformations.
//!
//! # Why a thin layer?
//!
//! [`kimberlite_wire`] already exposes [`Frame::encode`] /
//! [`Frame::decode`] and [`Message::to_frame`] /
//! [`Message::from_frame`]. The sync client used those inline; the
//! async client cannot reach into the same `&mut self` state, so we
//! lift the two operations into named functions that both clients
//! call. The functions stay zero-cost wrappers; their value is the
//! single named entry point + the audit-trail comment that ties the
//! sites together.

use bytes::BytesMut;
use kimberlite_wire::{Frame, Message, Request};

use crate::error::ClientResult;

/// Encode a [`Request`] into the supplied write buffer.
///
/// Wraps the request in [`Message::Request`], converts to a
/// [`Frame`], and appends its bytes. The buffer is left in a state
/// that an `AsyncWrite::write_all` (or sync `write_all`) can flush
/// to the socket directly.
///
/// # Errors
///
/// Returns [`ClientError::Wire`] if framing fails — typically only
/// when the request payload exceeds the wire's maximum frame size.
pub fn encode_request(request: &Request, write_buf: &mut BytesMut) -> ClientResult<()> {
    let frame = Message::Request(request.clone()).to_frame()?;
    frame.encode(write_buf);
    Ok(())
}

/// Try to decode a single [`Frame`] from the head of `read_buf`.
///
/// Returns:
/// - `Ok(Some(frame))` — a complete frame was consumed from
///   `read_buf` (its bytes are removed from the buffer).
/// - `Ok(None)` — buffer holds fewer bytes than a full frame; the
///   caller should read more from the socket and try again.
/// - `Err(_)` — the buffer head looks like a frame but the wire
///   layer rejected it (length-prefix corruption, etc.).
///
/// # Errors
///
/// Returns [`ClientError::Wire`] when [`Frame::decode`] fails; the
/// caller typically treats this as a fatal protocol error and
/// closes the connection.
pub fn decode_frame(read_buf: &mut BytesMut) -> ClientResult<Option<Frame>> {
    Ok(Frame::decode(read_buf)?)
}

/// Decode a [`Frame`] into a [`Message`].
///
/// Pure conversion; the framing pair `decode_frame` +
/// `decode_message` is what [`Client::read_message`] and
/// [`AsyncClient`]'s reader task both call.
///
/// # Errors
///
/// Returns [`ClientError::Wire`] for a malformed payload.
pub fn decode_message(frame: &Frame) -> ClientResult<Message> {
    Ok(Message::from_frame(frame)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_types::TenantId;
    use kimberlite_wire::{HandshakeRequest, RequestId, RequestPayload, PROTOCOL_VERSION};

    fn sample_request() -> Request {
        Request::new(
            RequestId::new(7),
            TenantId::new(1),
            RequestPayload::Handshake(HandshakeRequest {
                client_version: PROTOCOL_VERSION,
                auth_token: None,
            }),
        )
    }

    #[test]
    fn encode_then_decode_roundtrip_yields_same_request() {
        let req = sample_request();
        let mut buf = BytesMut::new();
        encode_request(&req, &mut buf).expect("encode");
        let frame = decode_frame(&mut buf).expect("decode").expect("complete");
        let msg = decode_message(&frame).expect("decode message");
        match msg {
            Message::Request(decoded) => {
                assert_eq!(decoded.id.0, req.id.0);
                assert_eq!(u64::from(decoded.tenant_id), u64::from(req.tenant_id));
            }
            other => panic!("expected Request, got {other:?}"),
        }
    }

    #[test]
    fn decode_partial_buffer_returns_none() {
        let req = sample_request();
        let mut buf = BytesMut::new();
        encode_request(&req, &mut buf).expect("encode");
        // Truncate to 3 bytes — far less than any real frame header.
        buf.truncate(3);
        let result = decode_frame(&mut buf).expect("decode short ok");
        assert!(
            result.is_none(),
            "partial frame must yield None, not Err or premature Some"
        );
    }

    #[test]
    fn encode_appends_to_existing_buffer() {
        let req = sample_request();
        let mut buf = BytesMut::from(&b"prefix"[..]);
        encode_request(&req, &mut buf).expect("encode");
        assert!(buf.starts_with(b"prefix"), "encode must not truncate prefix");
        assert!(buf.len() > 6, "encode must append framed bytes");
    }
}
