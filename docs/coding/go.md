---
title: "Go Client"
section: "coding"
slug: "go"
order: 4
---

# Go Client

Build Kimberlite applications in Go with simplicity and performance.

## Prerequisites

- Go 1.21 or later
- Running Kimberlite cluster (see [Start](/docs/start))

## Install

```bash
go get github.com/kimberlitedb/kimberlite-go
```

## Quick Verification

Create `main.go`:

```go
package main

import (
    "fmt"
    kimberlite "github.com/kimberlitedb/kimberlite-go"
)

func main() {
    fmt.Println("Kimberlite Go client imported successfully!")
    fmt.Printf("Version: %s\n", kimberlite.Version)
}
```

Run it:

```bash
go run main.go
```

## Sample Projects

### Basic: Create Table and Query Data

```go
package main

import (
    "context"
    "fmt"
    "log"

    kimberlite "github.com/kimberlitedb/kimberlite-go"
)

func main() {
    ctx := context.Background()

    // Connect to cluster
    client, err := kimberlite.Connect(ctx, &kimberlite.Config{
        Addresses: []string{"localhost:3000"},
    })
    if err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    // Create table
    _, err = client.Exec(ctx, `
        CREATE TABLE users (
            id INT PRIMARY KEY,
            email TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )
    `)
    if err != nil {
        log.Fatal(err)
    }

    // Insert data
    _, err = client.Exec(ctx,
        "INSERT INTO users (id, email) VALUES (?, ?)",
        1, "alice@example.com",
    )
    if err != nil {
        log.Fatal(err)
    }

    // Query data
    rows, err := client.Query(ctx, "SELECT * FROM users")
    if err != nil {
        log.Fatal(err)
    }
    defer rows.Close()

    for rows.Next() {
        var id int
        var email string
        var createdAt time.Time
        if err := rows.Scan(&id, &email, &createdAt); err != nil {
            log.Fatal(err)
        }
        fmt.Printf("User %d: %s\n", id, email)
    }
}
```

### Compliance: Enable RBAC and Test Access Control

```go
package main

import (
    "context"
    "fmt"
    "log"

    kimberlite "github.com/kimberlitedb/kimberlite-go"
)

func main() {
    ctx := context.Background()

    // Connect as admin
    adminClient, err := kimberlite.Connect(ctx, &kimberlite.Config{
        Addresses: []string{"localhost:3000"},
        User:      "admin",
        Password:  "admin-password",
    })
    if err != nil {
        log.Fatal(err)
    }
    defer adminClient.Close()

    // Create role with limited permissions
    _, err = adminClient.Exec(ctx, `
        CREATE ROLE data_analyst;
        GRANT SELECT ON patients TO data_analyst;
    `)
    if err != nil {
        log.Fatal(err)
    }

    // Create user with role
    _, err = adminClient.Exec(ctx, `
        CREATE USER analyst1
        WITH PASSWORD 'analyst-password'
        WITH ROLE data_analyst;
    `)
    if err != nil {
        log.Fatal(err)
    }

    // Connect as analyst
    analystClient, err := kimberlite.Connect(ctx, &kimberlite.Config{
        Addresses: []string{"localhost:3000"},
        User:      "analyst1",
        Password:  "analyst-password",
    })
    if err != nil {
        log.Fatal(err)
    }
    defer analystClient.Close()

    // This works (SELECT granted)
    rows, err := analystClient.Query(ctx, "SELECT * FROM patients")
    if err != nil {
        log.Fatal(err)
    }
    defer rows.Close()

    count := 0
    for rows.Next() {
        count++
    }
    fmt.Printf("Found %d patients\n", count)

    // This fails (no INSERT permission)
    _, err = analystClient.Exec(ctx,
        "INSERT INTO patients VALUES (99, 'Unauthorized', '000-00-0000')",
    )
    if err != nil {
        if kimberlite.IsPermissionDenied(err) {
            fmt.Printf("Access denied: %v\n", err)
        } else {
            log.Fatal(err)
        }
    }
}
```

### Multi-Tenant: Tenant Isolation Example

```go
package main

import (
    "context"
    "fmt"
    "log"

    kimberlite "github.com/kimberlitedb/kimberlite-go"
)

func main() {
    ctx := context.Background()

    // Connect to tenant 1
    tenant1Client, err := kimberlite.Connect(ctx, &kimberlite.Config{
        Addresses: []string{"localhost:3000"},
        TenantID:  1,
    })
    if err != nil {
        log.Fatal(err)
    }
    defer tenant1Client.Close()

    // Create data for tenant 1
    _, err = tenant1Client.Exec(ctx, `
        CREATE TABLE orders (id INT, customer TEXT, amount DECIMAL);
        INSERT INTO orders VALUES (1, 'Alice', 99.99);
    `)
    if err != nil {
        log.Fatal(err)
    }

    // Connect to tenant 2
    tenant2Client, err := kimberlite.Connect(ctx, &kimberlite.Config{
        Addresses: []string{"localhost:3000"},
        TenantID:  2,
    })
    if err != nil {
        log.Fatal(err)
    }
    defer tenant2Client.Close()

    // Tenant 2 cannot see tenant 1's data
    rows, err := tenant2Client.Query(ctx, "SELECT * FROM orders")
    if err != nil {
        log.Fatal(err)
    }
    defer rows.Close()

    count := 0
    for rows.Next() {
        count++
    }
    fmt.Printf("Tenant 2 sees %d orders\n", count) // 0

    // Create separate data for tenant 2
    _, err = tenant2Client.Exec(ctx, "INSERT INTO orders VALUES (1, 'Bob', 149.99)")
    if err != nil {
        log.Fatal(err)
    }

    rows, err = tenant2Client.Query(ctx, "SELECT * FROM orders")
    if err != nil {
        log.Fatal(err)
    }
    defer rows.Close()

    count = 0
    for rows.Next() {
        count++
    }
    fmt.Printf("Tenant 2 sees %d orders\n", count) // 1
}
```

### Compliance: Data Classification and Masking

```go
package main

import (
    "context"
    "fmt"
    "log"

    kimberlite "github.com/kimberlitedb/kimberlite-go"
)

func main() {
    ctx := context.Background()

    client, err := kimberlite.Connect(ctx, &kimberlite.Config{
        Addresses: []string{"localhost:3000"},
    })
    if err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    // Create table with PHI data
    _, err = client.Exec(ctx, `
        CREATE TABLE patients (
            id INT PRIMARY KEY,
            name TEXT NOT NULL,
            ssn TEXT NOT NULL,
            diagnosis TEXT
        );
    `)
    if err != nil {
        log.Fatal(err)
    }

    // Classify sensitive columns
    _, err = client.Exec(ctx,
        "ALTER TABLE patients MODIFY COLUMN ssn SET CLASSIFICATION 'PHI'",
    )
    if err != nil {
        log.Fatal(err)
    }

    _, err = client.Exec(ctx,
        "ALTER TABLE patients MODIFY COLUMN diagnosis SET CLASSIFICATION 'MEDICAL'",
    )
    if err != nil {
        log.Fatal(err)
    }

    // Insert data
    _, err = client.Exec(ctx, `
        INSERT INTO patients VALUES
            (1, 'Alice Johnson', '123-45-6789', 'Hypertension'),
            (2, 'Bob Smith', '987-65-4321', 'Diabetes');
    `)
    if err != nil {
        log.Fatal(err)
    }

    // Create masking rule
    _, err = client.Exec(ctx, "CREATE MASK ssn_mask ON patients.ssn USING REDACT")
    if err != nil {
        log.Fatal(err)
    }

    // Query - SSN is automatically masked
    rows, err := client.Query(ctx, "SELECT * FROM patients")
    if err != nil {
        log.Fatal(err)
    }
    defer rows.Close()

    for rows.Next() {
        var id int
        var name, ssn, diagnosis string
        if err := rows.Scan(&id, &name, &ssn, &diagnosis); err != nil {
            log.Fatal(err)
        }
        fmt.Printf("%s: SSN=%s\n", name, ssn) // SSN shows as ****
    }

    // View classifications
    rows, err = client.Query(ctx, "SHOW CLASSIFICATIONS FOR patients")
    if err != nil {
        log.Fatal(err)
    }
    defer rows.Close()

    for rows.Next() {
        var column, classification string
        if err := rows.Scan(&column, &classification); err != nil {
            log.Fatal(err)
        }
        fmt.Printf("%s: %s\n", column, classification)
    }
}
```

### Time-Travel: Query Historical State

```go
package main

import (
    "context"
    "fmt"
    "log"
    "time"

    kimberlite "github.com/kimberlitedb/kimberlite-go"
)

func main() {
    ctx := context.Background()

    client, err := kimberlite.Connect(ctx, &kimberlite.Config{
        Addresses: []string{"localhost:3000"},
    })
    if err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    // Insert initial data
    _, err = client.Exec(ctx, `
        CREATE TABLE inventory (product_id INT, quantity INT);
        INSERT INTO inventory VALUES (1, 100);
    `)
    if err != nil {
        log.Fatal(err)
    }

    // Wait a moment
    time.Sleep(2 * time.Second)
    checkpoint := time.Now()

    // Update inventory
    _, err = client.Exec(ctx, "UPDATE inventory SET quantity = 75 WHERE product_id = 1")
    if err != nil {
        log.Fatal(err)
    }

    // Query current state
    rows, err := client.Query(ctx, "SELECT * FROM inventory WHERE product_id = 1")
    if err != nil {
        log.Fatal(err)
    }
    defer rows.Close()

    if rows.Next() {
        var productID, quantity int
        if err := rows.Scan(&productID, &quantity); err != nil {
            log.Fatal(err)
        }
        fmt.Printf("Current quantity: %d\n", quantity) // 75
    }

    // Query historical state
    rows, err = client.Query(ctx,
        "SELECT * FROM inventory AS OF TIMESTAMP ? WHERE product_id = 1",
        checkpoint,
    )
    if err != nil {
        log.Fatal(err)
    }
    defer rows.Close()

    if rows.Next() {
        var productID, quantity int
        if err := rows.Scan(&productID, &quantity); err != nil {
            log.Fatal(err)
        }
        fmt.Printf("Historical quantity: %d\n", quantity) // 100
    }
}
```

## API Reference

### Creating a Client

```go
import kimberlite "github.com/kimberlitedb/kimberlite-go"

// Basic connection
client, err := kimberlite.Connect(ctx, &kimberlite.Config{
    Addresses: []string{"localhost:3000"},
})

// With authentication
client, err := kimberlite.Connect(ctx, &kimberlite.Config{
    Addresses: []string{"localhost:3000"},
    User:      "username",
    Password:  "password",
})

// With tenant isolation
client, err := kimberlite.Connect(ctx, &kimberlite.Config{
    Addresses: []string{"localhost:3000"},
    TenantID:  1,
})

// With TLS
client, err := kimberlite.Connect(ctx, &kimberlite.Config{
    Addresses:  []string{"localhost:3000"},
    TLSEnabled: true,
    TLSCACert:  "/path/to/ca.pem",
})

// Full configuration
client, err := kimberlite.Connect(ctx, &kimberlite.Config{
    Addresses:     []string{"localhost:3000", "localhost:3001", "localhost:3002"},
    User:          "admin",
    Password:      "password",
    TenantID:      1,
    TLSEnabled:    true,
    TLSCACert:     "/path/to/ca.pem",
    TLSClientCert: "/path/to/client.pem",
    TLSClientKey:  "/path/to/client-key.pem",
    Timeout:       5 * time.Second,
    MaxRetries:    3,
})
```

### Executing Queries

```go
// DDL (CREATE, ALTER, DROP)
_, err := client.Exec(ctx, `
    CREATE TABLE products (
        id INT PRIMARY KEY,
        name TEXT NOT NULL,
        price DECIMAL
    )
`)

// DML (INSERT, UPDATE, DELETE)
result, err := client.Exec(ctx,
    "INSERT INTO products VALUES (?, ?, ?)",
    1, "Widget", 19.99,
)
rowsAffected, _ := result.RowsAffected()
fmt.Printf("Inserted %d rows\n", rowsAffected)
```

### Querying Data

```go
// Simple query
rows, err := client.Query(ctx, "SELECT * FROM products")
if err != nil {
    log.Fatal(err)
}
defer rows.Close()

for rows.Next() {
    var id int
    var name string
    var price float64
    if err := rows.Scan(&id, &name, &price); err != nil {
        log.Fatal(err)
    }
    fmt.Printf("%s: $%.2f\n", name, price)
}

// Parameterized query
rows, err := client.Query(ctx,
    "SELECT * FROM products WHERE price > ?",
    25.0,
)

// QueryRow for single result
var count int
err := client.QueryRow(ctx, "SELECT COUNT(*) FROM products").Scan(&count)
if err != nil {
    log.Fatal(err)
}
fmt.Printf("Product count: %d\n", count)
```

### Transactions

```go
// Begin transaction
tx, err := client.Begin(ctx)
if err != nil {
    log.Fatal(err)
}

// Execute in transaction
_, err = tx.Exec(ctx, "UPDATE accounts SET balance = balance - 100 WHERE id = 1")
if err != nil {
    tx.Rollback(ctx)
    log.Fatal(err)
}

_, err = tx.Exec(ctx, "UPDATE accounts SET balance = balance + 100 WHERE id = 2")
if err != nil {
    tx.Rollback(ctx)
    log.Fatal(err)
}

// Commit transaction
if err := tx.Commit(ctx); err != nil {
    log.Fatal(err)
}
```

### Error Handling

```go
import "github.com/kimberlitedb/kimberlite-go"

_, err := client.Exec(ctx, "INSERT INTO users VALUES (1, 'alice@example.com')")
if err != nil {
    switch {
    case kimberlite.IsConnectionError(err):
        fmt.Println("Failed to connect to cluster")
    case kimberlite.IsAuthenticationError(err):
        fmt.Println("Invalid credentials")
    case kimberlite.IsPermissionDenied(err):
        fmt.Println("No permission for this operation")
    case kimberlite.IsConstraintViolation(err):
        fmt.Printf("Constraint violation: %v\n", err)
    case kimberlite.IsQueryError(err):
        fmt.Printf("Query error: %v\n", err)
    default:
        fmt.Printf("Error: %v\n", err)
    }
}
```

### Prepared Statements

```go
// Prepare statement
stmt, err := client.Prepare(ctx,
    "INSERT INTO logs (timestamp, message) VALUES (?, ?)",
)
if err != nil {
    log.Fatal(err)
}
defer stmt.Close()

// Execute multiple times
_, err = stmt.Exec(ctx, time.Now(), "User logged in")
if err != nil {
    log.Fatal(err)
}

_, err = stmt.Exec(ctx, time.Now(), "User logged out")
if err != nil {
    log.Fatal(err)
}
```

### Working with Types

```go
import (
    "database/sql"
    "time"
    "github.com/shopspring/decimal"
)

// Insert with proper types
_, err := client.Exec(ctx, `
    INSERT INTO transactions (id, amount, timestamp, metadata)
    VALUES (?, ?, ?, ?)
`,
    1,
    decimal.NewFromFloat(99.99),
    time.Now(),
    `{"source": "web", "ip": "192.168.1.1"}`,
)

// Query with typed extraction
rows, err := client.Query(ctx, "SELECT * FROM transactions WHERE id = 1")
if err != nil {
    log.Fatal(err)
}
defer rows.Close()

if rows.Next() {
    var id int
    var amount decimal.Decimal
    var timestamp time.Time
    var metadata string

    if err := rows.Scan(&id, &amount, &timestamp, &metadata); err != nil {
        log.Fatal(err)
    }

    fmt.Printf("Amount: %s\n", amount)
    fmt.Printf("Timestamp: %s\n", timestamp)
    fmt.Printf("Metadata: %s\n", metadata)
}
```

## Testing

Use the standard Go testing package:

```go
package main

import (
    "context"
    "testing"

    kimberlite "github.com/kimberlitedb/kimberlite-go"
)

func setup(t *testing.T) *kimberlite.Client {
    ctx := context.Background()

    client, err := kimberlite.Connect(ctx, &kimberlite.Config{
        Addresses: []string{"localhost:3000"},
    })
    if err != nil {
        t.Fatal(err)
    }

    // Clean database
    rows, err := client.Query(ctx, "SHOW TABLES")
    if err != nil {
        t.Fatal(err)
    }
    defer rows.Close()

    for rows.Next() {
        var name string
        if err := rows.Scan(&name); err != nil {
            t.Fatal(err)
        }
        _, err = client.Exec(ctx, "DROP TABLE IF EXISTS "+name)
        if err != nil {
            t.Fatal(err)
        }
    }

    return client
}

func TestCreateTable(t *testing.T) {
    client := setup(t)
    defer client.Close()

    ctx := context.Background()

    _, err := client.Exec(ctx, `
        CREATE TABLE test_table (
            id INT PRIMARY KEY,
            name TEXT NOT NULL
        )
    `)
    if err != nil {
        t.Fatal(err)
    }

    rows, err := client.Query(ctx, "SHOW TABLES")
    if err != nil {
        t.Fatal(err)
    }
    defer rows.Close()

    found := false
    for rows.Next() {
        var name string
        if err := rows.Scan(&name); err != nil {
            t.Fatal(err)
        }
        if name == "test_table" {
            found = true
            break
        }
    }

    if !found {
        t.Error("Table test_table not found")
    }
}

func TestInsertAndQuery(t *testing.T) {
    client := setup(t)
    defer client.Close()

    ctx := context.Background()

    _, err := client.Exec(ctx, "CREATE TABLE users (id INT, email TEXT)")
    if err != nil {
        t.Fatal(err)
    }

    _, err = client.Exec(ctx,
        "INSERT INTO users VALUES (?, ?)",
        1, "test@example.com",
    )
    if err != nil {
        t.Fatal(err)
    }

    rows, err := client.Query(ctx, "SELECT * FROM users WHERE id = 1")
    if err != nil {
        t.Fatal(err)
    }
    defer rows.Close()

    if !rows.Next() {
        t.Fatal("Expected 1 row, got 0")
    }

    var id int
    var email string
    if err := rows.Scan(&id, &email); err != nil {
        t.Fatal(err)
    }

    if email != "test@example.com" {
        t.Errorf("Expected email 'test@example.com', got '%s'", email)
    }
}
```

## Examples

Complete example applications are available in the repository:

- `examples/go/basic/` - Simple CRUD application
- `examples/go/compliance/` - HIPAA-compliant healthcare app
- `examples/go/multi-tenant/` - Multi-tenant SaaS application
- `examples/go/http/` - HTTP REST API with net/http
- `examples/go/grpc/` - gRPC service

## Next Steps

- [Python Client](/docs/coding/python) - Python SDK
- [TypeScript Client](/docs/coding/typescript) - Node.js SDK
- [Rust Client](/docs/coding/rust) - Native Rust SDK
- [CLI Reference](/docs/reference/cli) - Command-line tools
- [SQL Reference](/docs/reference/sql/overview) - SQL dialect

## Further Reading

- [SDK Architecture](/docs/reference/sdk/overview) - How SDKs work
- [Protocol Specification](/docs/reference/protocol) - Wire protocol details
- [Compliance Guide](/docs/concepts/compliance) - 23 frameworks explained
- [RBAC Guide](/docs/concepts/rbac) - Role-based access control
