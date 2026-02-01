# Rust SDK Quickstart

Get started with Kimberlite in Rust in under 5 minutes.

## Installation

Add Kimberlite to your `Cargo.toml`:

```toml
[dependencies]
kimberlite = "0.1"
kimberlite-types = "0.1"
```

## Basic Usage

### 1. Connect to Kimberlite

```rust
use kimberlite::Kimberlite;
use kmb_types::{TenantId, DataClass};

// Open local instance
let db = Kimberlite::open("./data")?;
let tenant = db.tenant(TenantId::new(1));
```

### 2. Create a Stream

```rust
let stream_id = tenant.create_stream("events", DataClass::NonPHI)?;
println!("Created stream: {}", stream_id);
```

### 3. Append Events

```rust
let events = vec![
    b"event1".to_vec(),
    b"event2".to_vec(),
    b"event3".to_vec(),
];

let offset = tenant.append(stream_id, events)?;
println!("Appended events starting at offset: {}", offset);
```

### 4. Read Events

```rust
use kmb_types::Offset;

let events = tenant.read_events(
    stream_id,
    Offset::new(0),
    1024 * 1024, // 1 MB
)?;

for event in events {
    println!("Event: {:?}", event);
}
```

### 5. Query with SQL

```rust
let results = tenant.query(
    "SELECT * FROM events WHERE id = ?",
    &[1.into()],
)?;

for row in results.rows {
    println!("{:?}", row);
}
```

## Complete Example

```rust
use kimberlite::Kimberlite;
use kmb_types::{TenantId, DataClass, Offset};

fn main() -> anyhow::Result<()> {
    // Open database
    let db = Kimberlite::open("./data")?;
    let tenant = db.tenant(TenantId::new(1));

    // Create stream
    let stream_id = tenant.create_stream("patient_events", DataClass::PHI)?;

    // Append events
    let events = vec![
        br#"{"type": "admission", "patient_id": "P123"}"#.to_vec(),
        br#"{"type": "diagnosis", "patient_id": "P123", "code": "I10"}"#.to_vec(),
    ];
    let offset = tenant.append(stream_id, events)?;

    // Read back
    let read_events = tenant.read_events(stream_id, offset, 1024)?;
    println!("Read {} events", read_events.len());

    // Sync to disk
    tenant.sync()?;

    Ok(())
}
```

## Common Patterns

### Point-in-Time Queries

```rust
use kmb_types::Offset;

let historical = tenant.query_at(
    "SELECT * FROM patients WHERE id = ?",
    &[123.into()],
    Offset::new(5000), // Query as of offset 5000
)?;
```

### Error Handling

```rust
use kimberlite::KimberliteError;

match tenant.create_stream("events", DataClass::PHI) {
    Ok(stream_id) => println!("Created: {}", stream_id),
    Err(KimberliteError::StreamAlreadyExists { .. }) => {
        println!("Stream already exists, continuing...");
    }
    Err(e) => return Err(e.into()),
}
```

### Batch Operations

```rust
// Batch append for better performance
let large_batch: Vec<Vec<u8>> = (0..1000)
    .map(|i| format!("event_{}", i).into_bytes())
    .collect();

let offset = tenant.append(stream_id, large_batch)?;
```

## Next Steps

- [Architecture Overview](../ARCHITECTURE.md)
- [API Documentation](https://docs.rs/kimberlite)
- [Examples](../../examples/rust/)
- [Testing Guide](../TESTING.md)
