---
title: "Python Client"
section: "coding"
slug: "python"
order: 1
---

# Python Client

Build Kimberlite applications in Python.

## Prerequisites

- Python 3.8 or later
- pip package manager
- Running Kimberlite cluster (see [Start](/docs/start))

## Install

```bash
pip install kimberlite
```

For development:

```bash
pip install kimberlite[dev]
```

## Quick Verification

Create a file `test.py`:

```python
import kimberlite
print("Kimberlite Python client imported successfully!")
print(f"Version: {kimberlite.__version__}")
```

Run it:

```bash
python test.py
```

## Sample Projects

### Basic: Create Table and Query Data

```python
from kimberlite import Client

# Connect to cluster
client = Client(addresses=["localhost:3000"])

# Create table
client.execute("""
    CREATE TABLE users (
        id INT PRIMARY KEY,
        email TEXT NOT NULL,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    )
""")

# Insert data
client.execute(
    "INSERT INTO users (id, email) VALUES (?, ?)",
    (1, "alice@example.com")
)

# Query data
result = client.query("SELECT * FROM users")
for row in result:
    print(f"User {row['id']}: {row['email']}")

client.close()
```

### Compliance: Enable RBAC and Test Access Control

```python
from kimberlite import Client, PermissionDeniedError

# Connect as admin
admin_client = Client(
    addresses=["localhost:3000"],
    user="admin",
    password="admin-password"
)

# Create role with limited permissions
admin_client.execute("""
    CREATE ROLE data_analyst;
    GRANT SELECT ON patients TO data_analyst;
""")

# Create user with role
admin_client.execute("""
    CREATE USER analyst1
    WITH PASSWORD 'analyst-password'
    WITH ROLE data_analyst;
""")

admin_client.close()

# Connect as analyst (limited permissions)
analyst_client = Client(
    addresses=["localhost:3000"],
    user="analyst1",
    password="analyst-password"
)

# This works (SELECT granted)
result = analyst_client.query("SELECT * FROM patients")
print(f"Found {len(result)} patients")

# This fails (no INSERT permission)
try:
    analyst_client.execute(
        "INSERT INTO patients VALUES (99, 'Unauthorized', '000-00-0000')"
    )
except PermissionDeniedError as e:
    print(f"Access denied: {e}")

analyst_client.close()
```

### Multi-Tenant: Tenant Isolation Example

```python
from kimberlite import Client

# Connect to tenant 1
tenant1_client = Client(
    addresses=["localhost:3000"],
    tenant_id=1
)

# Create data for tenant 1
tenant1_client.execute("""
    CREATE TABLE orders (id INT, customer TEXT, amount DECIMAL);
    INSERT INTO orders VALUES (1, 'Alice', 99.99);
""")

# Connect to tenant 2
tenant2_client = Client(
    addresses=["localhost:3000"],
    tenant_id=2
)

# Tenant 2 cannot see tenant 1's data
result = tenant2_client.query("SELECT * FROM orders")
print(f"Tenant 2 sees {len(result)} orders")  # Output: 0

# Create separate data for tenant 2
tenant2_client.execute("INSERT INTO orders VALUES (1, 'Bob', 149.99)")
result = tenant2_client.query("SELECT * FROM orders")
print(f"Tenant 2 sees {len(result)} orders")  # Output: 1

tenant1_client.close()
tenant2_client.close()
```

### Compliance: Data Classification and Masking

```python
from kimberlite import Client

client = Client(addresses=["localhost:3000"])

# Create table with PHI data
client.execute("""
    CREATE TABLE patients (
        id INT PRIMARY KEY,
        name TEXT NOT NULL,
        ssn TEXT NOT NULL,
        diagnosis TEXT
    );
""")

# Classify sensitive columns
client.execute("ALTER TABLE patients MODIFY COLUMN ssn SET CLASSIFICATION 'PHI'")
client.execute("ALTER TABLE patients MODIFY COLUMN diagnosis SET CLASSIFICATION 'MEDICAL'")

# Insert data
client.execute("""
    INSERT INTO patients VALUES
        (1, 'Alice Johnson', '123-45-6789', 'Hypertension'),
        (2, 'Bob Smith', '987-65-4321', 'Diabetes');
""")

# Create masking rule
client.execute("CREATE MASK ssn_mask ON patients.ssn USING REDACT")

# Query - SSN is automatically masked
result = client.query("SELECT * FROM patients")
for row in result:
    print(f"{row['name']}: SSN={row['ssn']}")  # SSN shows as ****

# View classifications
classifications = client.query("SHOW CLASSIFICATIONS FOR patients")
for cls in classifications:
    print(f"{cls['column']}: {cls['classification']}")

client.close()
```

### Time-Travel: Query Historical State

```python
from kimberlite import Client
from datetime import datetime, timedelta

client = Client(addresses=["localhost:3000"])

# Insert initial data
client.execute("""
    CREATE TABLE inventory (product_id INT, quantity INT);
    INSERT INTO inventory VALUES (1, 100);
""")

# Wait a moment
import time
time.sleep(2)
checkpoint = datetime.now()

# Update inventory
client.execute("UPDATE inventory SET quantity = 75 WHERE product_id = 1")

# Query current state
result = client.query("SELECT * FROM inventory WHERE product_id = 1")
print(f"Current quantity: {result[0]['quantity']}")  # 75

# Query historical state
result = client.query(
    "SELECT * FROM inventory AS OF TIMESTAMP ? WHERE product_id = 1",
    (checkpoint,)
)
print(f"Historical quantity: {result[0]['quantity']}")  # 100

client.close()
```

## API Reference

### Creating a Client

```python
from kimberlite import Client

# Basic connection
client = Client(addresses=["localhost:3000"])

# With authentication
client = Client(
    addresses=["localhost:3000"],
    user="username",
    password="password"
)

# With tenant isolation
client = Client(
    addresses=["localhost:3000"],
    tenant_id=1
)

# With TLS
client = Client(
    addresses=["localhost:3000"],
    tls_enabled=True,
    tls_ca_cert="/path/to/ca.pem"
)

# Using context manager (recommended)
with Client(addresses=["localhost:3000"]) as client:
    # Use client
    pass
# Automatically closed
```

### Executing Queries

```python
# DDL (CREATE, ALTER, DROP)
client.execute("""
    CREATE TABLE products (
        id INT PRIMARY KEY,
        name TEXT NOT NULL,
        price DECIMAL
    )
""")

# DML (INSERT, UPDATE, DELETE)
rows_affected = client.execute(
    "INSERT INTO products VALUES (?, ?, ?)",
    (1, "Widget", 19.99)
)
print(f"Inserted {rows_affected} rows")

# Batch insert
rows = [
    (2, "Gadget", 29.99),
    (3, "Doohickey", 39.99)
]
client.execute_many(
    "INSERT INTO products VALUES (?, ?, ?)",
    rows
)
```

### Querying Data

```python
# Simple query
result = client.query("SELECT * FROM products")
for row in result:
    print(f"{row['name']}: ${row['price']}")

# Parameterized query
result = client.query(
    "SELECT * FROM products WHERE price > ?",
    (25.0,)
)

# Query with dictionary result
result = client.query("SELECT * FROM products", as_dict=True)
for row in result:
    print(row)  # {'id': 1, 'name': 'Widget', 'price': 19.99}

# Query with tuple result
result = client.query("SELECT * FROM products", as_dict=False)
for row in result:
    print(row)  # (1, 'Widget', 19.99)

# Streaming large results
for row in client.query_stream("SELECT * FROM large_table"):
    process(row)
```

### Transactions

```python
# Explicit transaction
client.begin()
try:
    client.execute("UPDATE accounts SET balance = balance - 100 WHERE id = 1")
    client.execute("UPDATE accounts SET balance = balance + 100 WHERE id = 2")
    client.commit()
except Exception as e:
    client.rollback()
    raise

# Context manager (automatic rollback on exception)
with client.transaction():
    client.execute("UPDATE accounts SET balance = balance - 100 WHERE id = 1")
    client.execute("UPDATE accounts SET balance = balance + 100 WHERE id = 2")
# Automatically committed
```

### Error Handling

```python
from kimberlite import (
    ConnectionError,
    AuthenticationError,
    PermissionDeniedError,
    QueryError,
    ConstraintViolationError
)

try:
    client = Client(addresses=["localhost:3000"])
    client.execute("INSERT INTO users VALUES (1, 'alice@example.com')")
except ConnectionError:
    print("Failed to connect to cluster")
except AuthenticationError:
    print("Invalid credentials")
except PermissionDeniedError:
    print("No permission for this operation")
except ConstraintViolationError as e:
    print(f"Constraint violation: {e}")
except QueryError as e:
    print(f"Query error: {e}")
```

### Prepared Statements

```python
# Prepare statement (compiled once, executed many times)
stmt = client.prepare("INSERT INTO logs (timestamp, message) VALUES (?, ?)")

# Execute multiple times
stmt.execute((datetime.now(), "User logged in"))
stmt.execute((datetime.now(), "User logged out"))

# Batch execution
rows = [
    (datetime.now(), "Event 1"),
    (datetime.now(), "Event 2"),
    (datetime.now(), "Event 3")
]
stmt.execute_many(rows)

stmt.close()
```

### Working with Types

```python
from kimberlite import types
from datetime import datetime
from decimal import Decimal

# Insert with proper types
client.execute("""
    INSERT INTO transactions (
        id,
        amount,
        timestamp,
        metadata
    ) VALUES (?, ?, ?, ?)
""", (
    1,
    Decimal("99.99"),
    datetime.now(),
    {"source": "web", "ip": "192.168.1.1"}
))

# Query and extract typed values
result = client.query("SELECT * FROM transactions WHERE id = 1")
row = result[0]

print(f"Amount: {row['amount']}")  # Decimal('99.99')
print(f"Timestamp: {row['timestamp']}")  # datetime object
print(f"Metadata: {row['metadata']}")  # dict
```

### Async Support

```python
import asyncio
from kimberlite.aio import AsyncClient

async def main():
    # Create async client
    client = AsyncClient(addresses=["localhost:3000"])

    # Execute query
    result = await client.query("SELECT * FROM users")
    for row in result:
        print(row)

    # Close connection
    await client.close()

# Run async code
asyncio.run(main())

# Or use context manager
async def main():
    async with AsyncClient(addresses=["localhost:3000"]) as client:
        result = await client.query("SELECT * FROM users")
        print(f"Found {len(result)} users")

asyncio.run(main())
```

## Testing

Use pytest for testing with Kimberlite:

```python
import pytest
from kimberlite import Client

@pytest.fixture
def client():
    """Provide test client."""
    client = Client(addresses=["localhost:3000"])
    yield client
    client.close()

@pytest.fixture
def clean_database(client):
    """Start with clean database."""
    # Drop all tables
    tables = client.query("SHOW TABLES")
    for table in tables:
        client.execute(f"DROP TABLE IF EXISTS {table['name']}")

def test_create_table(client, clean_database):
    """Test table creation."""
    client.execute("""
        CREATE TABLE test_table (
            id INT PRIMARY KEY,
            name TEXT NOT NULL
        )
    """)

    tables = client.query("SHOW TABLES")
    assert any(t['name'] == 'test_table' for t in tables)

def test_insert_and_query(client, clean_database):
    """Test insert and query."""
    client.execute("CREATE TABLE users (id INT, email TEXT)")
    client.execute("INSERT INTO users VALUES (1, 'test@example.com')")

    result = client.query("SELECT * FROM users WHERE id = 1")
    assert len(result) == 1
    assert result[0]['email'] == 'test@example.com'
```

## Examples

Complete example applications are available in the repository:

- `examples/python/basic/` - Simple CRUD application
- `examples/python/compliance/` - HIPAA-compliant healthcare app
- `examples/python/multi-tenant/` - Multi-tenant SaaS application
- `examples/python/time-travel/` - Historical queries and audit trails
- `examples/python/async/` - Async/await patterns

## Next Steps

- [TypeScript Client](/docs/coding/typescript) - Node.js SDK
- [Rust Client](/docs/coding/rust) - Native Rust SDK
- [Go Client](/docs/coding/go) - Go SDK
- [CLI Reference](/docs/reference/cli) - Command-line tools
- [SQL Reference](/docs/reference/sql/overview) - SQL dialect

## Further Reading

- [SDK Architecture](/docs/reference/sdk/overview) - How SDKs work
- [Protocol Specification](/docs/reference/protocol) - Wire protocol details
- [Compliance Guide](/docs/concepts/compliance) - 23 frameworks explained
- [RBAC Guide](/docs/concepts/rbac) - Role-based access control
