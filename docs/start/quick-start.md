---
title: "Quick Start"
section: "start"
slug: "quick-start"
order: 2
---

# Quick Start

Get Kimberlite running and execute your first SQL queries in 5 minutes.

## Prerequisites

- `kimberlite` binary installed — see [Installation](installation.md)

## Step 1: Start the Dev Environment

```bash
kimberlite dev
```

This starts:
- **Database server** at `127.0.0.1:5432`
- **Studio UI** at `http://localhost:5555`

Open [http://localhost:5555](http://localhost:5555) in your browser to see the Studio UI.

## Step 2: Open the REPL

In a new terminal:

```bash
kimberlite repl --tenant 1
```

You should see the Kimberlite REPL prompt:

```
Kimberlite v0.4.0 — Compliance-First Database
Connected to 127.0.0.1:5432 | Tenant: dev-fixtures (1)
Type .help for available commands

kimberlite>
```

## Step 3: Create a Table

```sql
CREATE TABLE patients (
    id BIGINT,
    name TEXT,
    dob TEXT,
    PRIMARY KEY (id)
);
```

## Step 4: Insert Data

```sql
INSERT INTO patients VALUES (1, 'Jane Doe', '1980-01-15');
INSERT INTO patients VALUES (2, 'John Smith', '1992-07-22');
INSERT INTO patients VALUES (3, 'Alice Johnson', '1975-03-08');
```

## Step 5: Query Data

```sql
-- All patients
SELECT * FROM patients;

-- Single patient by ID (point lookup)
SELECT * FROM patients WHERE id = 1;

-- Filter by name pattern
SELECT * FROM patients WHERE name LIKE 'J%';
```

Expected output:
```
id | name         | dob
---+--------------+------------
1  | Jane Doe     | 1980-01-15
2  | John Smith   | 1992-07-22
3  | Alice Johnson| 1975-03-08

3 rows (1.2ms)
```

## Step 6: Time-Travel Query

Kimberlite stores every change as an immutable log entry. Query the database at any past state:

```sql
-- Query at the current state (latest)
SELECT * FROM patients;

-- Insert a new record
INSERT INTO patients VALUES (4, 'Bob Williams', '1988-11-30');

-- Query at offset 3 (before the insert)
SELECT * FROM patients AT OFFSET 3;
```

## What's Next

- **[First Application](first-app.md)** — Build a healthcare compliance app
- **[SQL Reference](../reference/sql.md)** — Full SQL syntax
- **[Concepts](../concepts/)** — Why Kimberlite works this way
