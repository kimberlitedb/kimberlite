# Docker Examples

Docker configurations for running Kimberlite.

## Single Node

Run a single Kimberlite node:

```bash
docker-compose up -d
```

This starts Kimberlite on port 5432.

## Three-Node Cluster

Run a 3-node replicated cluster:

```bash
docker-compose -f docker-compose.cluster.yml up -d
```

This starts:
- Node 0 on port 5432
- Node 1 on port 5433
- Node 2 on port 5434

## Connecting

```bash
# Connect to single node
kimberlite repl --address 127.0.0.1:5432

# Connect to cluster (any node)
kimberlite repl --address 127.0.0.1:5432
```

## Stopping

```bash
# Single node
docker-compose down

# Cluster
docker-compose -f docker-compose.cluster.yml down
```

## Data Persistence

Data is stored in Docker volumes:
- `kimberlite-data` for single node
- `kimberlite-data-0`, `kimberlite-data-1`, `kimberlite-data-2` for cluster

To remove all data:

```bash
docker-compose down -v
```
