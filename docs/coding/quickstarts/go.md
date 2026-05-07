---
title: "Go SDK Quickstart"
section: "coding/quickstarts"
slug: "go"
order: 4
---

# Go SDK Quickstart

**Status**: 📋 Planned (v0.9.0 Phase 1). Rust / TypeScript / Python SDKs are at full parity today — see the [SDK parity matrix](/docs/reference/sdk/parity).

The Go SDK ships in three phases:

- **Phase 1 (v0.9.0):** `Connect`, `Query`, `Append`, `Read`, `Subscribe`, `Pool` over the FFI bridge that the Rust / Python / TypeScript SDKs share. Scaffolding lives at `sdks/go/` in the repo.
- **Phase 2 (v0.9.x → v1.0):** compliance surface (`consent.*`, `erasure.*`, `audit.*`, `export_subject`, `breach_*`).
- **Phase 3 (v1.0 gate):** typed primitives, framework integrations, and parity sign-off in the matrix.

Until then, Go services can call Kimberlite via the wire protocol directly (see [protocol reference](/docs/reference/protocol)) or via gRPC bridges. The remainder of this page is a forward-looking sketch of the Go API.

## Installation

```bash
go get github.com/kimberlitedb/kimberlite-go
```

## Basic Usage

### 1. Connect to Kimberlite

```go
import "github.com/kimberlitedb/kimberlite-go"

client, err := kimberlite.Connect(kimberlite.Config{
    Addresses: []string{"localhost:5432"},
    TenantID:  1,
    AuthToken: "your-token",
})
if err != nil {
    return err
}
defer client.Close()
```

### 2. Create a Stream

```go
streamID, err := client.CreateStream(ctx, "events", kimberlite.DataClassPHI)
if err != nil {
    return err
}
fmt.Printf("Created stream: %d\n", streamID)
```

### 3. Append Events

```go
events := [][]byte{
    []byte(`{"type": "admission", "patient_id": "P123"}`),
    []byte(`{"type": "diagnosis", "patient_id": "P123", "code": "I10"}`),
}

offset, err := client.Append(ctx, streamID, events)
if err != nil {
    return err
}
fmt.Printf("Appended %d events at offset %d\n", len(events), offset)
```

### 4. Read Events

```go
events, err := client.Read(ctx, streamID, kimberlite.ReadOptions{
    FromOffset: 0,
    MaxBytes:   1024 * 1024, // 1 MB
})
if err != nil {
    return err
}

for _, event := range events {
    fmt.Printf("Offset %d: %s\n", event.Offset, event.Data)
}
```

## Complete Example

```go
package main

import (
    "context"
    "fmt"
    "log"

    "github.com/kimberlitedb/kimberlite-go"
)

func main() {
    ctx := context.Background()

    // Connect
    client, err := kimberlite.Connect(kimberlite.Config{
        Addresses: []string{"localhost:5432"},
        TenantID:  1,
        AuthToken: "development-token",
    })
    if err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    // Create stream
    streamID, err := client.CreateStream(ctx, "patient_events", kimberlite.DataClassPHI)
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Created stream: %d\n", streamID)

    // Append events
    events := [][]byte{
        []byte(`{"type": "admission", "patient_id": "P123"}`),
        []byte(`{"type": "diagnosis", "patient_id": "P123", "code": "I10"}`),
    }
    offset, err := client.Append(ctx, streamID, events)
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Appended %d events at offset %d\n", len(events), offset)

    // Read back
    readEvents, err := client.Read(ctx, streamID, kimberlite.ReadOptions{
        FromOffset: offset,
        MaxBytes:   1024,
    })
    if err != nil {
        log.Fatal(err)
    }

    for _, event := range readEvents {
        fmt.Printf("  %d: %s\n", event.Offset, event.Data)
    }
}
```

## Common Patterns

### Error Handling

```go
import "errors"

streamID, err := client.CreateStream(ctx, "events", kimberlite.DataClassPHI)
if err != nil {
    if errors.Is(err, kimberlite.ErrPermissionDenied) {
        log.Println("No permission for PHI data")
        return nil
    }
    return err
}
```

### Context Cancellation

```go
ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
defer cancel()

events, err := client.Read(ctx, streamID, kimberlite.ReadOptions{
    FromOffset: 0,
    MaxBytes:   1024 * 1024,
})
```

### Batch Processing

```go
offset := uint64(0)
batchSize := uint64(1024 * 1024)

for {
    events, err := client.Read(ctx, streamID, kimberlite.ReadOptions{
        FromOffset: offset,
        MaxBytes:   batchSize,
    })
    if err != nil {
        return err
    }
    if len(events) == 0 {
        break
    }

    // Process batch
    for _, event := range events {
        if err := processEvent(event); err != nil {
            return err
        }
    }

    offset = events[len(events)-1].Offset + 1
}
```

## Next Steps

- [SDK Architecture](/docs/reference/sdk/overview)
- [Protocol Specification](/docs/reference/protocol)
- Go examples will land alongside the v0.9.0 Phase 1 SDK in `examples/go/`. Until then, see [Rust](/docs/coding/quickstarts/rust), [TypeScript](/docs/coding/quickstarts/typescript), or [Python](/docs/coding/quickstarts/python) for working SDK examples.
