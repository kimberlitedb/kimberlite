//! Info command - show server information.

use anyhow::{Context, Result};
use kmb_client::{Client, ClientConfig};
use kmb_types::TenantId;

pub fn run(server: &str, tenant: u64) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let _client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;

    println!("Connection Information");
    println!("----------------------");
    println!("Server:    {server}");
    println!("Tenant ID: {tenant}");
    println!("Status:    Connected");

    Ok(())
}
