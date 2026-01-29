# Quickstart Example

This example demonstrates the basics of using Kimberlite.

## Prerequisites

- Kimberlite CLI binary (download from releases or build from source)

## Steps

### 1. Initialize the Data Directory

```bash
./kimberlite init ./data --development
```

### 2. Start the Server

```bash
./kimberlite start --address 3000 ./data
```

### 3. Connect with the REPL

In a new terminal:

```bash
./kimberlite repl --address 127.0.0.1:3000
```

### 4. Try Some Queries

```sql
-- Create a table
CREATE TABLE patients (id BIGINT, name TEXT, created_at TIMESTAMP);

-- Insert data
INSERT INTO patients VALUES (1, 'Jane Doe', NULL);
INSERT INTO patients VALUES (2, 'John Smith', NULL);

-- Query data
SELECT * FROM patients;
SELECT * FROM patients WHERE id = 1;
```

### 5. Run Sample Queries

You can also run the sample queries file:

```bash
# Execute all sample queries
for query in $(cat sample-queries.sql | grep -v '^--' | grep -v '^$'); do
  ./kimberlite query --server 127.0.0.1:3000 "$query"
done
```

## Using the Init Script

For convenience, you can use the init script:

```bash
./init.sh
```

This will:
1. Initialize a data directory
2. Start the server in the background
3. Wait for the server to be ready
4. Execute sample queries

## Clean Up

To clean up:

```bash
# Stop the server (Ctrl+C if running in foreground)
# Remove data directory
rm -rf ./data
```
