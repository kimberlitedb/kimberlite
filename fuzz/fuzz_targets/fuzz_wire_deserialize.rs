#![no_main]

use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Arbitrary input direct to FrameHeader::decode — raw parse surface,
    // must never panic.
    {
        let mut header_buf = data;
        let _ = kmb_wire::FrameHeader::decode(&mut header_buf);
    }

    // Arbitrary input to Frame::decode. If a frame is accepted, then:
    //   (a) re-encoding the frame produces exactly the prefix of the original
    //       buffer that Frame::decode consumed — proving the accepted frame
    //       is a fixed point of the codec; and
    //   (b) Request::from_frame / Response::from_frame do not panic. If either
    //       succeeds, the postcard encode→decode round-trip is stable: the
    //       second encoding must byte-match the first.
    let mut buf = BytesMut::from(data);
    let original_len = buf.len();

    match kmb_wire::Frame::decode(&mut buf) {
        Ok(Some(frame)) => {
            let consumed = original_len - buf.len();
            let reencoded = frame.encode_to_bytes();
            assert_eq!(
                reencoded.len(),
                consumed,
                "decoded frame must re-encode to exactly the consumed prefix"
            );
            assert_eq!(
                reencoded.as_ref(),
                &data[..consumed],
                "decoded frame must round-trip to identical bytes"
            );

            if let Ok(req) = kmb_wire::Request::from_frame(&frame) {
                let reframed = req
                    .to_frame()
                    .expect("a request decoded from a valid frame must re-serialize");
                let redecoded = kmb_wire::Request::from_frame(&reframed)
                    .expect("request round-trip must be stable");
                let reframed2 = redecoded.to_frame().expect("second encode must succeed");
                assert_eq!(
                    reframed.payload.as_ref(),
                    reframed2.payload.as_ref(),
                    "Request serialization must be deterministic"
                );
            }

            if let Ok(resp) = kmb_wire::Response::from_frame(&frame) {
                let reframed = resp
                    .to_frame()
                    .expect("a response decoded from a valid frame must re-serialize");
                let redecoded = kmb_wire::Response::from_frame(&reframed)
                    .expect("response round-trip must be stable");
                let reframed2 = redecoded.to_frame().expect("second encode must succeed");
                assert_eq!(
                    reframed.payload.as_ref(),
                    reframed2.payload.as_ref(),
                    "Response serialization must be deterministic"
                );
            }
        }
        Ok(None) => {
            // Incomplete frame — buf must be unchanged.
            assert_eq!(
                buf.len(),
                original_len,
                "incomplete decode must not consume bytes"
            );
        }
        Err(_) => {
            // Decode error is fine; we only require no panic.
        }
    }
});
