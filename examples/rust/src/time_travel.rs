//! Time travel query example.
//!
//! Demonstrates querying historical state using point-in-time queries.
//!
//! # Running
//!
//! ```bash
//! cargo run --example time_travel
//! ```

use anyhow::Result;
use kmb_client::{Client, ClientConfig};
use kmb_types::{Offset, TenantId};

fn main() -> Result<()> {
    println!("Kimberlite Time Travel Example");
    println!("==============================\n");

    // Connect to the server
    let server = "127.0.0.1:3000";
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(1);

    println!("Connecting to {}...", server);
    let mut client = Client::connect(server, tenant_id, config)?;
    println!("Connected!\n");

    // Create a table
    println!("Setting up data...");
    let _ = client.query("CREATE TABLE inventory (id BIGINT, item TEXT, quantity BIGINT)", &[]);

    // Insert initial data
    client.query("INSERT INTO inventory VALUES (1, 'Widget', 100)", &[])?;
    println!("Inserted: Widget with quantity 100");

    // Query current state
    println!("\nCurrent state:");
    let result = client.query("SELECT * FROM inventory WHERE id = 1", &[])?;
    print_result(&result);

    // Make some changes
    println!("\nMaking changes...");
    client.query("INSERT INTO inventory VALUES (2, 'Gadget', 50)", &[])?;
    println!("Inserted: Gadget with quantity 50");

    // Query current state again
    println!("\nCurrent state (after changes):");
    let result = client.query("SELECT * FROM inventory", &[])?;
    print_result(&result);

    // Time travel query - see historical state
    // Note: You need to know the log position to query at
    // In practice, you'd track positions for audit purposes
    println!("\nTime travel query at position 1:");
    let result = client.query_at("SELECT * FROM inventory", &[], Offset::new(1))?;
    print_result(&result);

    println!("\nTime travel allows you to:");
    println!("  - Reconstruct state at any point in history");
    println!("  - Audit what data looked like during incidents");
    println!("  - Generate compliance reports for specific dates");

    Ok(())
}

fn print_result(result: &kmb_wire::QueryResponse) {
    if result.rows.is_empty() {
        println!("  (no rows)");
        return;
    }

    for row in &result.rows {
        let values: Vec<String> = row
            .iter()
            .map(|v| match v {
                kmb_wire::QueryValue::Null => "NULL".to_string(),
                kmb_wire::QueryValue::BigInt(n) => n.to_string(),
                kmb_wire::QueryValue::Text(s) => s.clone(),
                kmb_wire::QueryValue::Boolean(b) => b.to_string(),
                kmb_wire::QueryValue::Timestamp(t) => t.to_string(),
            })
            .collect();
        println!("  {}", values.join(" | "));
    }
}
