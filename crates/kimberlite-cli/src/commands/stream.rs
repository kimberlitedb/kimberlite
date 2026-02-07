//! Stream management commands.

use anyhow::{Context, Result, bail};
use kimberlite_client::{Client, ClientConfig};
use kimberlite_types::{DataClass, Offset, StreamId, TenantId};

use crate::style::{
    colors::SemanticStyle, create_spinner, finish_and_clear, finish_success, print_code_example,
    print_hint, print_labeled, print_spacer, print_success, print_warn,
};

/// Creates a new stream.
pub fn create(server: &str, tenant: u64, name: &str, class: &str) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let sp = create_spinner(&format!("Connecting to {server}..."));
    let mut client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;
    finish_and_clear(&sp);

    let data_class = parse_data_class(class)?;

    let sp = create_spinner(&format!("Creating stream '{name}'..."));
    let stream_id = client.create_stream(name, data_class)?;
    finish_success(&sp, &format!("Created stream '{name}'"));

    print_spacer();
    print_labeled("Stream ID", &u64::from(stream_id).to_string());
    print_labeled("Name", name);
    print_labeled("Classification", class);

    print_spacer();
    print_hint("Append events:");
    print_code_example(&format!(
        "kimberlite stream append {} '{{\"event\": \"data\"}}'",
        u64::from(stream_id)
    ));

    Ok(())
}

/// Lists all streams (requires server-side support).
pub fn list(server: &str, tenant: u64) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let sp = create_spinner(&format!("Connecting to {server}..."));
    let _client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;
    finish_success(&sp, &format!("Connected to {server}"));

    print_spacer();
    print_labeled("Server", server);
    print_labeled("Tenant ID", &tenant.to_string());

    print_spacer();
    // TODO(v0.7.0): Stream listing requires server-side support
    print_warn("Stream listing requires server-side support (not yet implemented).");
    print_hint("Use when available:");
    print_code_example("kimberlite query \"SELECT * FROM _streams\"");

    Ok(())
}

/// Appends events to a stream.
pub fn append(server: &str, tenant: u64, stream_id: u64, events: Vec<String>) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let sp = create_spinner(&format!("Connecting to {server}..."));
    let mut client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;
    finish_and_clear(&sp);

    let stream = StreamId::new(stream_id);
    let event_count = events.len();
    let event_data: Vec<Vec<u8>> = events.into_iter().map(String::into_bytes).collect();

    let sp = create_spinner(&format!("Appending {event_count} event(s)..."));
    let offset = client.append(stream, event_data, Offset::ZERO)?;
    finish_success(
        &sp,
        &format!(
            "Appended {event_count} event(s) at offset {}",
            offset.as_u64()
        ),
    );

    print_spacer();
    print_labeled("Stream ID", &stream_id.to_string());
    print_labeled("Offset", &offset.as_u64().to_string());

    Ok(())
}

/// Reads events from a stream.
pub fn read(server: &str, tenant: u64, stream_id: u64, from: u64, max_bytes: u64) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let sp = create_spinner(&format!("Connecting to {server}..."));
    let mut client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;
    finish_and_clear(&sp);

    let stream = StreamId::new(stream_id);
    let from_offset = Offset::new(from);

    let sp = create_spinner(&format!("Reading from offset {from}..."));
    let response = client.read_events(stream, from_offset, max_bytes)?;
    finish_and_clear(&sp);

    if response.events.is_empty() {
        print_warn(&format!("No events found starting from offset {from}."));
    } else {
        print_success(&format!("Read {} event(s):", response.events.len()));
        print_spacer();

        for (i, event) in response.events.iter().enumerate() {
            let offset_num = from + i as u64;
            let text = String::from_utf8_lossy(event);
            println!("  {} {}", format!("[{offset_num}]").muted(), text);
        }
    }

    if let Some(next) = response.next_offset {
        print_spacer();
        print_hint(&format!("Next offset: {}", next.as_u64()));
    }

    Ok(())
}

/// Parses a data classification string.
fn parse_data_class(s: &str) -> Result<DataClass> {
    match s.to_lowercase().as_str() {
        "non-phi" | "nonphi" => Ok(DataClass::Public),
        "phi" => Ok(DataClass::PHI),
        "deidentified" | "de-identified" => Ok(DataClass::Deidentified),
        other => bail!("Unknown data class: '{other}'. Use: non-phi, phi, or deidentified."),
    }
}
