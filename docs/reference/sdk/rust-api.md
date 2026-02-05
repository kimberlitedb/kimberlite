# Rust API Reference

Native Rust SDK for Kimberlite.

**Crate:** `kimberlite`  
**Status:** Stable (v0.4.0)  
**MSRV:** 1.88

## Installation

```toml
[dependencies]
kimberlite = "0.4"
tokio = { version = "1", features = ["full"] }
```

## Client

See [docs.rs/kimberlite](https://docs.rs/kimberlite) for complete API documentation.

### Quick Reference

```rust
use kimberlite::{Client, TenantId, StreamId, Position};

// Connect
let client = Client::connect("localhost:7000").await?;

// Append
let position = client.append(
    TenantId::new(1),
    StreamId::new(1, 100),
    b"event data"
).await?;

// Read
let events = client.read_stream(
    TenantId::new(1),
    StreamId::new(1, 100)
).await?;

// Subscribe
let mut subscription = client.subscribe(
    TenantId::new(1),
    StreamId::new(1, 100)
).await?;

while let Some(event) = subscription.next().await {
    println!("Event: {:?}", event);
}
```

## Examples

See [Rust Quickstart](../../coding/quickstarts/rust.md) for complete examples.

See [docs.rs/kimberlite](https://docs.rs/kimberlite/latest/kimberlite/) for full API documentation.
