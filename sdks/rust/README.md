# Kimberlite Rust SDK

**Status**: ✅ Ready

The Rust SDK is the native implementation located in the `crates/kimberlite` workspace.

## Installation

```toml
[dependencies]
kimberlite = "0.1"
```

## Quick Start

```rust
use kimberlite::{Kimberlite, DataClass};
use kmb_types::{TenantId, StreamId};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Kimberlite::open("./data")?;
    let tenant = db.tenant(TenantId::new(1));

    // Create stream
    let stream_id = tenant.create_stream("events", DataClass::NonPHI)?;

    // Append events
    let events = vec![b"event1".to_vec(), b"event2".to_vec()];
    let offset = tenant.append(stream_id, events)?;

    // Query
    let results = tenant.query("SELECT * FROM events WHERE id = ?", &[1.into()])?;

    Ok(())
}
```

## Documentation

- [API Documentation](https://docs.rs/kimberlite)
- [Examples](../../examples/rust/)
- [Architecture](../../docs/ARCHITECTURE.md)

## Features

- Zero-copy reads
- Type-safe API
- Native performance
- Full access to all Kimberlite features
