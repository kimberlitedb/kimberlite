# Kimberlite + FastAPI example

Minimal FastAPI app using the Kimberlite Python SDK.

## Prerequisites

- A running Kimberlite server on `127.0.0.1:5432` (run `just dev` from the
  repo root).
- Python 3.11+ with `pip install -r requirements.txt`.
- The Kimberlite FFI library built:  `cargo build -p kimberlite-ffi --release`.

## Run

```bash
uvicorn main:app --reload
```

## Demo

```bash
# Grant consent for a patient.
curl -X POST http://localhost:8000/patients \
  -H 'Content-Type: application/json' \
  -d '{"name": "alice", "consent_purpose": "Analytics"}'

# Check consent.
curl http://localhost:8000/patients/alice
```
