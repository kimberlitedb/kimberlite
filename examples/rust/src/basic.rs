//! Basic Kimberlite client example.
//!
//! Demonstrates connecting to a server and executing queries.
//!
//! # Running
//!
//! ```bash
//! # Start a server first
//! kimberlite init ./data --development
//! kimberlite start --address 3000 ./data
//!
//! # Run this example
//! cargo run --example basic
//! ```

use anyhow::Result;
use kimberlite_client::{Client, ClientConfig, QueryValue};
use kimberlite_types::TenantId;

fn main() -> Result<()> {
    println!("Kimberlite Basic Example");
    println!("========================\n");

    // Connect to the server
    let server = "127.0.0.1:3000";
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(1);

    println!("Connecting to {}...", server);
    let mut client = Client::connect(server, tenant_id, config)?;
    println!("Connected!\n");

    // Create a table
    println!("Creating table...");
    let _ = client.query("CREATE TABLE users (id BIGINT, name TEXT, email TEXT)", &[]);
    println!("Table created.\n");

    // Insert some data
    println!("Inserting data...");
    client.query("INSERT INTO users VALUES (1, 'Alice', 'alice@example.com')", &[])?;
    client.query("INSERT INTO users VALUES (2, 'Bob', 'bob@example.com')", &[])?;
    client.query("INSERT INTO users VALUES (3, 'Charlie', 'charlie@example.com')", &[])?;
    println!("Data inserted.\n");

    // Query the data
    println!("Querying data...");
    let result = client.query("SELECT * FROM users", &[])?;

    // Print results
    println!("\nResults:");
    println!("{}", "-".repeat(50));

    // Print headers
    println!("{}", result.columns.join(" | "));
    println!("{}", "-".repeat(50));

    // Print rows
    for row in &result.rows {
        let values: Vec<String> = row
            .iter()
            .map(|v| match v {
                QueryValue::Null => "NULL".to_string(),
                QueryValue::BigInt(n) => n.to_string(),
                QueryValue::Text(s) => s.clone(),
                QueryValue::Boolean(b) => b.to_string(),
                QueryValue::Timestamp(t) => t.to_string(),
            })
            .collect();
        println!("{}", values.join(" | "));
    }

    println!("{}", "-".repeat(50));
    println!("\n{} row(s) returned", result.rows.len());

    Ok(())
}
