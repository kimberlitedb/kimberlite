# Kimberlite + Django example

Minimal Django app using the Kimberlite Python SDK for consent management.

## Prerequisites

- A running Kimberlite server on `127.0.0.1:5432`.
- Python 3.11+, Django 5+.
- `cargo build -p kimberlite-ffi --release` to build the native library.

## Run

```bash
pip install -r requirements.txt
python manage.py runserver
```

## Endpoints

- `GET /api/health/` — liveness
- `POST /api/patients/` — grant consent
- `GET /api/patients/<subject_id>/` — check Analytics consent

See `views.py` for the integration pattern. The pool is initialized once
at module load via `kimberlite_init.py` and shared across requests.
