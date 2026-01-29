//! Stream management commands.

use anyhow::{Context, Result, bail};
use kmb_client::{Client, ClientConfig};
use kmb_types::{DataClass, Offset, StreamId, TenantId};

/// Creates a new stream.
pub fn create(server: &str, tenant: u64, name: &str, class: &str) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let mut client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;

    let data_class = parse_data_class(class)?;
    let stream_id = client.create_stream(name, data_class)?;

    println!("Created stream '{name}' with ID: {}", u64::from(stream_id));
    println!();
    println!("To append events:");
    println!(
        "  kimberlite stream append {} '{{\"event\": \"data\"}}'",
        u64::from(stream_id)
    );

    Ok(())
}

/// Lists all streams (requires server-side support).
pub fn list(server: &str, tenant: u64) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let _client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;

    println!("Connected to: {server}");
    println!("Tenant ID:    {tenant}");
    println!();
    println!("Note: Stream listing requires server-side support (not yet implemented).");
    println!("Use 'kimberlite query \"SELECT * FROM _streams\"' when available.");

    Ok(())
}

/// Appends events to a stream.
pub fn append(server: &str, tenant: u64, stream_id: u64, events: Vec<String>) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let mut client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;

    let stream = StreamId::new(stream_id);
    let event_data: Vec<Vec<u8>> = events.into_iter().map(String::into_bytes).collect();

    let offset = client.append(stream, event_data)?;
    println!("Appended at offset: {}", offset.as_u64());

    Ok(())
}

/// Reads events from a stream.
pub fn read(server: &str, tenant: u64, stream_id: u64, from: u64, max_bytes: u64) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let mut client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;

    let stream = StreamId::new(stream_id);
    let from_offset = Offset::new(from);

    let response = client.read_events(stream, from_offset, max_bytes)?;

    if response.events.is_empty() {
        println!("No events found starting from offset {from}.");
    } else {
        println!("Events ({}):", response.events.len());
        for (i, event) in response.events.iter().enumerate() {
            let text = String::from_utf8_lossy(event);
            println!("  [{}] {}", from + i as u64, text);
        }
    }

    if let Some(next) = response.next_offset {
        println!();
        println!("Next offset: {}", next.as_u64());
    }

    Ok(())
}

/// Parses a data classification string.
fn parse_data_class(s: &str) -> Result<DataClass> {
    match s.to_lowercase().as_str() {
        "non-phi" | "nonphi" => Ok(DataClass::NonPHI),
        "phi" => Ok(DataClass::PHI),
        "deidentified" | "de-identified" => Ok(DataClass::Deidentified),
        other => bail!("Unknown data class: '{other}'. Use: non-phi, phi, or deidentified."),
    }
}
