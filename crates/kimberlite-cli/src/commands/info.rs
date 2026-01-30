//! Info command - show server information.

use anyhow::{Context, Result};
use kmb_client::{Client, ClientConfig};
use kmb_types::TenantId;

use crate::style::{
    create_spinner, finish_success, print_info_table, print_spacer,
};

pub fn run(server: &str, tenant: u64) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let sp = create_spinner(&format!("Connecting to {server}..."));
    let _client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;
    finish_success(&sp, "Connected");

    print_spacer();

    let entries = [
        ("Server", server),
        ("Tenant ID", &tenant.to_string()),
        ("Status", "Connected"),
    ];

    print_info_table(&entries);

    Ok(())
}
