//! Event streaming example.
//!
//! Demonstrates creating streams, appending events, and reading them back.
//!
//! # Running
//!
//! ```bash
//! cargo run --example streaming
//! ```

use anyhow::Result;
use kmb_client::{Client, ClientConfig};
use kmb_types::{DataClass, Offset, TenantId};

fn main() -> Result<()> {
    println!("Kimberlite Streaming Example");
    println!("============================\n");

    // Connect to the server
    let server = "127.0.0.1:3000";
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(1);

    println!("Connecting to {}...", server);
    let mut client = Client::connect(server, tenant_id, config)?;
    println!("Connected!\n");

    // Create a stream
    println!("Creating stream 'events'...");
    let stream_id = client.create_stream("events", DataClass::NonPHI)?;
    println!("Stream created with ID: {}\n", u64::from(stream_id));

    // Append events
    println!("Appending events...");

    let events = vec![
        r#"{"type": "user_created", "user_id": 1, "name": "Alice"}"#.as_bytes().to_vec(),
        r#"{"type": "user_updated", "user_id": 1, "email": "alice@new.com"}"#.as_bytes().to_vec(),
        r#"{"type": "user_deleted", "user_id": 1}"#.as_bytes().to_vec(),
    ];

    let offset = client.append(stream_id, events)?;
    println!("Events appended starting at offset: {}\n", offset.as_u64());

    // Read events back
    println!("Reading events from offset 0...");
    let response = client.read_events(stream_id, Offset::ZERO, 65536)?;

    println!("\nEvents ({}):", response.events.len());
    for (i, event) in response.events.iter().enumerate() {
        let text = String::from_utf8_lossy(event);
        println!("  [{}] {}", i, text);
    }

    if let Some(next) = response.next_offset {
        println!("\nNext offset: {}", next.as_u64());
    }

    Ok(())
}
