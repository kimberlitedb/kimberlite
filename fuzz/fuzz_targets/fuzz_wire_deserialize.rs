#![no_main]

use libfuzzer_sys::fuzz_target;
use bytes::BytesMut;

fuzz_target!(|data: &[u8]| {
    // Test frame-level deserialization
    let mut buf = BytesMut::from(data);

    // Try to decode a frame from arbitrary bytes
    // This tests:
    // - Header parsing robustness
    // - Magic number validation
    // - Protocol version checking
    // - Payload size limits (max 16 MiB)
    // - CRC32 checksum validation
    // - Buffer boundary conditions
    if let Ok(Some(frame)) = kmb_wire::Frame::decode(&mut buf) {
        // If we successfully decoded a frame, try to deserialize as Request
        // This tests:
        // - Bincode deserialization robustness
        // - Enum variant handling
        // - Field validation
        // - Nested structure handling
        let _request = kmb_wire::Request::from_frame(&frame);

        // Also try as Response
        let _response = kmb_wire::Response::from_frame(&frame);
    }

    // Even if frame decode fails, test edge cases by attempting direct
    // header decode without validation
    let mut header_buf = data;
    let _header = kmb_wire::FrameHeader::decode(&mut header_buf);
});
