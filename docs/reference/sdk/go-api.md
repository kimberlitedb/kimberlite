# Go API Reference

Go SDK for Kimberlite.

**Package:** `github.com/kimberlitedb/kimberlite-go`  
**Status:** Planned for v0.5.0  
**Go:** 1.21+

## Installation

```bash
go get github.com/kimberlitedb/kimberlite-go
```

## Client (Planned)

```go
package main

import (
    "context"
    "log"
    kmb "github.com/kimberlitedb/kimberlite-go"
)

func main() {
    // Connect
    client, err := kmb.Connect("localhost:7000")
    if err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    // Append
    position, err := client.Append(
        context.Background(),
        kmb.TenantID(1),
        kmb.StreamID{TenantID: 1, StreamNumber: 100},
        []byte("event data"),
    )
    if err != nil {
        log.Fatal(err)
    }

    // Read
    events, err := client.ReadStream(
        context.Background(),
        kmb.TenantID(1),
        kmb.StreamID{TenantID: 1, StreamNumber: 100},
    )
    if err != nil {
        log.Fatal(err)
    }

    for _, event := range events {
        log.Printf("Position: %d, Data: %s", event.Position, event.Data)
    }
}
```

## Status

Go SDK is planned for v0.5.0 (Q2 2024).

See [ROADMAP](../../../ROADMAP.md) for details.
