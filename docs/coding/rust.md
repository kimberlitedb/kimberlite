---
title: "Rust Client"
section: "coding"
slug: "rust"
order: 3
---

# Rust Client

Build Kimberlite applications in Rust with native performance and type safety.

## Prerequisites

- Rust 1.88 or later
- Cargo package manager
- Running Kimberlite cluster (see [Start](/docs/start))

## Install

Add to your `Cargo.toml`:

```toml
[dependencies]
kimberlite = "1.0"
tokio = { version = "1", features = ["full"] }
```

Or via command line:

```bash
cargo add kimberlite
cargo add tokio --features full
```

## Quick Verification

Create `src/main.rs`:

```rust
use kimberlite::Client;

fn main() {
    println!("Kimberlite Rust client imported successfully!");
    println!("Version: {}", kimberlite::VERSION);
}
```

Run it:

```bash
cargo run
```

## Sample Projects

### Basic: Create Table and Query Data

```rust
use kimberlite::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to cluster
    let client = Client::connect(&["localhost:3000"]).await?;

    // Create table
    client.execute(
        "CREATE TABLE users (
            id INT PRIMARY KEY,
            email TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )"
    ).await?;

    // Insert data
    client.execute_params(
        "INSERT INTO users (id, email) VALUES (?, ?)",
        &[&1, &"alice@example.com"]
    ).await?;

    // Query data
    let result = client.query("SELECT * FROM users").await?;
    for row in result {
        println!("User {}: {}", row.get::<i32>("id")?, row.get::<String>("email")?);
    }

    client.close().await?;
    Ok(())
}
```

### Compliance: Enable RBAC and Test Access Control

```rust
use kimberlite::{Client, Error};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect as admin
    let admin_client = Client::builder()
        .addresses(&["localhost:3000"])
        .user("admin")
        .password("admin-password")
        .build()
        .await?;

    // Create role with limited permissions
    admin_client.execute(
        "CREATE ROLE data_analyst;
         GRANT SELECT ON patients TO data_analyst;"
    ).await?;

    // Create user with role
    admin_client.execute(
        "CREATE USER analyst1
         WITH PASSWORD 'analyst-password'
         WITH ROLE data_analyst;"
    ).await?;

    admin_client.close().await?;

    // Connect as analyst
    let analyst_client = Client::builder()
        .addresses(&["localhost:3000"])
        .user("analyst1")
        .password("analyst-password")
        .build()
        .await?;

    // This works (SELECT granted)
    let result = analyst_client.query("SELECT * FROM patients").await?;
    println!("Found {} patients", result.len());

    // This fails (no INSERT permission)
    match analyst_client.execute(
        "INSERT INTO patients VALUES (99, 'Unauthorized', '000-00-0000')"
    ).await {
        Err(Error::PermissionDenied(msg)) => println!("Access denied: {}", msg),
        _ => panic!("Expected permission denied error")
    }

    analyst_client.close().await?;
    Ok(())
}
```

### Multi-Tenant: Tenant Isolation Example

```rust
use kimberlite::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to tenant 1
    let tenant1_client = Client::builder()
        .addresses(&["localhost:3000"])
        .tenant_id(1)
        .build()
        .await?;

    // Create data for tenant 1
    tenant1_client.execute(
        "CREATE TABLE orders (id INT, customer TEXT, amount DECIMAL);
         INSERT INTO orders VALUES (1, 'Alice', 99.99);"
    ).await?;

    // Connect to tenant 2
    let tenant2_client = Client::builder()
        .addresses(&["localhost:3000"])
        .tenant_id(2)
        .build()
        .await?;

    // Tenant 2 cannot see tenant 1's data
    let result = tenant2_client.query("SELECT * FROM orders").await?;
    println!("Tenant 2 sees {} orders", result.len()); // 0

    // Create separate data for tenant 2
    tenant2_client.execute("INSERT INTO orders VALUES (1, 'Bob', 149.99)").await?;
    let result = tenant2_client.query("SELECT * FROM orders").await?;
    println!("Tenant 2 sees {} orders", result.len()); // 1

    tenant1_client.close().await?;
    tenant2_client.close().await?;
    Ok(())
}
```

### Compliance: Data Classification and Masking

```rust
use kimberlite::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::connect(&["localhost:3000"]).await?;

    // Create table with PHI data
    client.execute(
        "CREATE TABLE patients (
            id INT PRIMARY KEY,
            name TEXT NOT NULL,
            ssn TEXT NOT NULL,
            diagnosis TEXT
        );"
    ).await?;

    // Classify sensitive columns
    client.execute(
        "ALTER TABLE patients MODIFY COLUMN ssn SET CLASSIFICATION 'PHI'"
    ).await?;
    client.execute(
        "ALTER TABLE patients MODIFY COLUMN diagnosis SET CLASSIFICATION 'MEDICAL'"
    ).await?;

    // Insert data
    client.execute(
        "INSERT INTO patients VALUES
            (1, 'Alice Johnson', '123-45-6789', 'Hypertension'),
            (2, 'Bob Smith', '987-65-4321', 'Diabetes');"
    ).await?;

    // Create masking rule
    client.execute("CREATE MASK ssn_mask ON patients.ssn USING REDACT").await?;

    // Query - SSN is automatically masked
    let result = client.query("SELECT * FROM patients").await?;
    for row in result {
        println!("{}: SSN={}",
            row.get::<String>("name")?,
            row.get::<String>("ssn")?  // Shows as ****
        );
    }

    // View classifications
    let classifications = client.query("SHOW CLASSIFICATIONS FOR patients").await?;
    for cls in classifications {
        println!("{}: {}",
            cls.get::<String>("column")?,
            cls.get::<String>("classification")?
        );
    }

    client.close().await?;
    Ok(())
}
```

### Time-Travel: Query Historical State

```rust
use kimberlite::Client;
use std::time::Duration;
use tokio::time;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::connect(&["localhost:3000"]).await?;

    // Insert initial data
    client.execute(
        "CREATE TABLE inventory (product_id INT, quantity INT);
         INSERT INTO inventory VALUES (1, 100);"
    ).await?;

    // Wait a moment
    time::sleep(Duration::from_secs(2)).await;
    let checkpoint = chrono::Utc::now();

    // Update inventory
    client.execute(
        "UPDATE inventory SET quantity = 75 WHERE product_id = 1"
    ).await?;

    // Query current state
    let result = client.query("SELECT * FROM inventory WHERE product_id = 1").await?;
    println!("Current quantity: {}", result[0].get::<i32>("quantity")?); // 75

    // Query historical state
    let result = client.query_params(
        "SELECT * FROM inventory AS OF TIMESTAMP ? WHERE product_id = 1",
        &[&checkpoint]
    ).await?;
    println!("Historical quantity: {}", result[0].get::<i32>("quantity")?); // 100

    client.close().await?;
    Ok(())
}
```

## API Reference

### Creating a Client

```rust
use kimberlite::Client;

// Basic connection
let client = Client::connect(&["localhost:3000"]).await?;

// With authentication
let client = Client::builder()
    .addresses(&["localhost:3000"])
    .user("username")
    .password("password")
    .build()
    .await?;

// With tenant isolation
let client = Client::builder()
    .addresses(&["localhost:3000"])
    .tenant_id(1)
    .build()
    .await?;

// With TLS
let client = Client::builder()
    .addresses(&["localhost:3000"])
    .tls_enabled(true)
    .tls_ca_cert("/path/to/ca.pem")
    .build()
    .await?;

// Full configuration
let client = Client::builder()
    .addresses(&["localhost:3000", "localhost:3001", "localhost:3002"])
    .user("admin")
    .password("password")
    .tenant_id(1)
    .tls_enabled(true)
    .tls_ca_cert("/path/to/ca.pem")
    .tls_client_cert("/path/to/client.pem")
    .tls_client_key("/path/to/client-key.pem")
    .timeout(Duration::from_secs(5))
    .max_retries(3)
    .build()
    .await?;
```

### Executing Queries

```rust
// DDL (CREATE, ALTER, DROP)
client.execute(
    "CREATE TABLE products (
        id INT PRIMARY KEY,
        name TEXT NOT NULL,
        price DECIMAL
    )"
).await?;

// DML (INSERT, UPDATE, DELETE)
let rows_affected = client.execute(
    "INSERT INTO products VALUES (?, ?, ?)",
    &[&1, &"Widget", &19.99]
).await?;
println!("Inserted {} rows", rows_affected);

// Batch insert
let rows = vec![
    vec![&2 as &dyn ToSql, &"Gadget", &29.99],
    vec![&3 as &dyn ToSql, &"Doohickey", &39.99],
];
client.execute_batch("INSERT INTO products VALUES (?, ?, ?)", rows).await?;
```

### Querying Data

```rust
// Simple query
let result = client.query("SELECT * FROM products").await?;
for row in result {
    println!("{}: ${}", row.get::<String>("name")?, row.get::<f64>("price")?);
}

// Parameterized query
let result = client.query_params(
    "SELECT * FROM products WHERE price > ?",
    &[&25.0]
).await?;

// Typed results with serde
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct Product {
    id: i32,
    name: String,
    price: f64,
}

let products: Vec<Product> = client.query_as("SELECT * FROM products").await?;
for product in products {
    println!("{}", product.name); // Type-safe access
}

// Streaming large results
let mut stream = client.query_stream("SELECT * FROM large_table").await?;
while let Some(row) = stream.next().await {
    process(row?);
}
```

### Transactions

```rust
// Explicit transaction
let mut tx = client.begin().await?;
tx.execute("UPDATE accounts SET balance = balance - 100 WHERE id = 1").await?;
tx.execute("UPDATE accounts SET balance = balance + 100 WHERE id = 2").await?;
tx.commit().await?;

// Rollback on error
let mut tx = client.begin().await?;
match tx.execute("UPDATE accounts SET balance = balance - 100 WHERE id = 1").await {
    Ok(_) => {
        tx.execute("UPDATE accounts SET balance = balance + 100 WHERE id = 2").await?;
        tx.commit().await?;
    }
    Err(e) => {
        tx.rollback().await?;
        return Err(e.into());
    }
}

// Automatic rollback with closure
client.transaction(|tx| async move {
    tx.execute("UPDATE accounts SET balance = balance - 100 WHERE id = 1").await?;
    tx.execute("UPDATE accounts SET balance = balance + 100 WHERE id = 2").await?;
    Ok(())
}).await?;
```

### Error Handling

```rust
use kimberlite::Error;

match client.execute("INSERT INTO users VALUES (1, 'alice@example.com')").await {
    Ok(rows) => println!("Inserted {} rows", rows),
    Err(Error::Connection(_)) => eprintln!("Failed to connect to cluster"),
    Err(Error::Authentication(_)) => eprintln!("Invalid credentials"),
    Err(Error::PermissionDenied(_)) => eprintln!("No permission for this operation"),
    Err(Error::ConstraintViolation(msg)) => eprintln!("Constraint violation: {}", msg),
    Err(Error::Query(msg)) => eprintln!("Query error: {}", msg),
    Err(e) => eprintln!("Error: {}", e),
}
```

### Prepared Statements

```rust
// Prepare statement
let stmt = client.prepare(
    "INSERT INTO logs (timestamp, message) VALUES (?, ?)"
).await?;

// Execute multiple times
stmt.execute(&[&chrono::Utc::now(), &"User logged in"]).await?;
stmt.execute(&[&chrono::Utc::now(), &"User logged out"]).await?;

// Batch execution
let rows = vec![
    vec![&chrono::Utc::now() as &dyn ToSql, &"Event 1"],
    vec![&chrono::Utc::now() as &dyn ToSql, &"Event 2"],
    vec![&chrono::Utc::now() as &dyn ToSql, &"Event 3"],
];
stmt.execute_batch(rows).await?;
```

### Working with Types

```rust
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde_json::Value as JsonValue;

// Insert with proper types
client.execute_params(
    "INSERT INTO transactions (id, amount, timestamp, metadata) VALUES (?, ?, ?, ?)",
    &[
        &1,
        &Decimal::new(9999, 2), // 99.99
        &Utc::now(),
        &serde_json::json!({"source": "web", "ip": "192.168.1.1"})
    ]
).await?;

// Query with typed extraction
let result = client.query("SELECT * FROM transactions WHERE id = 1").await?;
let row = &result[0];

let id: i32 = row.get("id")?;
let amount: Decimal = row.get("amount")?;
let timestamp: DateTime<Utc> = row.get("timestamp")?;
let metadata: JsonValue = row.get("metadata")?;

println!("Amount: {}", amount);
println!("Timestamp: {}", timestamp);
println!("Metadata: {}", metadata);
```

## Testing

Use the standard Rust testing framework:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite::Client;

    async fn setup() -> Client {
        let client = Client::connect(&["localhost:3000"]).await.unwrap();

        // Clean database
        let tables = client.query("SHOW TABLES").await.unwrap();
        for table in tables {
            let name: String = table.get("name").unwrap();
            client.execute(&format!("DROP TABLE IF EXISTS {}", name)).await.unwrap();
        }

        client
    }

    #[tokio::test]
    async fn test_create_table() {
        let client = setup().await;

        client.execute(
            "CREATE TABLE test_table (
                id INT PRIMARY KEY,
                name TEXT NOT NULL
            )"
        ).await.unwrap();

        let tables = client.query("SHOW TABLES").await.unwrap();
        assert!(tables.iter().any(|t| t.get::<String>("name").unwrap() == "test_table"));

        client.close().await.unwrap();
    }

    #[tokio::test]
    async fn test_insert_and_query() {
        let client = setup().await;

        client.execute("CREATE TABLE users (id INT, email TEXT)").await.unwrap();
        client.execute_params(
            "INSERT INTO users VALUES (?, ?)",
            &[&1, &"test@example.com"]
        ).await.unwrap();

        let result = client.query("SELECT * FROM users WHERE id = 1").await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].get::<String>("email").unwrap(), "test@example.com");

        client.close().await.unwrap();
    }
}
```

## Examples

Complete example applications are available in the repository:

- `examples/rust/basic/` - Simple CRUD application
- `examples/rust/compliance/` - HIPAA-compliant healthcare app
- `examples/rust/multi-tenant/` - Multi-tenant SaaS application
- `examples/rust/actix-web/` - Actix Web REST API
- `examples/rust/async/` - Async patterns and streaming

## Next Steps

- [Python Client](/docs/coding/python) - Python SDK
- [TypeScript Client](/docs/coding/typescript) - Node.js SDK
- [Go Client](/docs/coding/go) - Go SDK
- [CLI Reference](/docs/reference/cli) - Command-line tools
- [SQL Reference](/docs/reference/sql/overview) - SQL dialect

## Further Reading

- [SDK Architecture](/docs/reference/sdk/overview) - How SDKs work
- [Protocol Specification](/docs/reference/protocol) - Wire protocol details
- [Compliance Guide](/docs/concepts/compliance) - 23 frameworks explained
- [RBAC Guide](/docs/concepts/rbac) - Role-based access control
