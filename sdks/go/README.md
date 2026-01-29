# Kimberlite Go SDK

**Status**: 📋 Planned (Phase 11.5)

Idiomatic Go client for Kimberlite database.

## Installation

```bash
go get github.com/kimberlitedb/kimberlite-go
```

## Quick Start

```go
package main

import (
    "context"
    "log"

    "github.com/kimberlitedb/kimberlite-go"
)

func main() {
    client, err := kimberlite.Connect(kimberlite.Config{
        Addresses: []string{"localhost:5432"},
        TenantID:  1,
        AuthToken: "secret",
    })
    if err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    ctx := context.Background()

    // Create stream
    streamID, err := client.CreateStream(ctx, "events", kimberlite.DataClassPHI)
    if err != nil {
        log.Fatal(err)
    }

    // Append events
    events := [][]byte{
        []byte("event1"),
        []byte("event2"),
    }
    offset, err := client.Append(ctx, streamID, events)
    if err != nil {
        log.Fatal(err)
    }

    // Query
    rows, err := client.Query(ctx, "SELECT * FROM events WHERE timestamp > ?", 1704067200)
    if err != nil {
        log.Fatal(err)
    }
    defer rows.Close()

    for rows.Next() {
        var id int64
        var data []byte
        if err := rows.Scan(&id, &data); err != nil {
            log.Fatal(err)
        }
    }
}
```

## Features

- Context-aware cancellation
- `io.Closer` interface for `defer`
- SQL-style `Rows.Scan()` for queries
- No panics (explicit error returns)

## Documentation

- [Protocol Specification](../../docs/PROTOCOL.md)
- [SDK Architecture](../../docs/SDK.md)
