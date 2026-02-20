---
title: "Go SDK Quickstart"
section: "coding/quickstarts"
slug: "go"
order: 4
---

# Go SDK Quickstart

**Status**: 📋 Planned (Phase 11.5)

Get started with Kimberlite in Go (coming soon).

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

- [SDK Architecture](..//docs/reference/sdk/overview)
- [Protocol Specification](..//docs/reference/protocol)
- Go examples (coming soon)
