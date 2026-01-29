# Rust SDK Examples

Examples demonstrating how to use the Kimberlite Rust SDK.

## Prerequisites

- Rust 1.85+
- A running Kimberlite server

## Setup

1. Start a Kimberlite server:

```bash
kimberlite init ./data --development
kimberlite start --address 3000 ./data
```

2. Run an example:

```bash
cargo run --example basic
```

## Examples

| Example | Description |
|---------|-------------|
| `basic.rs` | Basic client connection and queries |
| `streaming.rs` | Event streaming with append and read |
| `time_travel.rs` | Point-in-time queries |

## Adding as a Dependency

Add to your `Cargo.toml`:

```toml
[dependencies]
kimberlite = "0.1"
kmb-client = "0.1"
```

## API Overview

```rust
use kmb_client::{Client, ClientConfig};
use kmb_types::TenantId;

// Connect to server
let config = ClientConfig::default();
let mut client = Client::connect("127.0.0.1:3000", TenantId::new(1), config)?;

// Execute queries
let result = client.query("SELECT * FROM patients", &[])?;

// Print results
for row in &result.rows {
    println!("{:?}", row);
}
```
