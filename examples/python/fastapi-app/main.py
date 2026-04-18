"""FastAPI integration example.

Run:
    pip install -r requirements.txt
    uvicorn main:app --reload

Endpoints:
    GET  /health
    POST /patients       {"name": "alice", "consent_purpose": "Analytics"}
    GET  /patients/{id}
    GET  /info
"""

from contextlib import asynccontextmanager
from typing import Optional

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

from kimberlite import Pool


# Shared pool, lazily created at startup.
_pool: Optional[Pool] = None


@asynccontextmanager
async def lifespan(app: FastAPI):
    global _pool
    _pool = Pool(address="127.0.0.1:5432", tenant_id=1, max_size=8)
    try:
        yield
    finally:
        _pool.shutdown()
        _pool = None


app = FastAPI(lifespan=lifespan)


class CreatePatient(BaseModel):
    name: str
    consent_purpose: str  # "Marketing" | "Analytics" | ...


@app.get("/health")
def health() -> dict:
    return {"status": "ok"}


@app.get("/info")
def info() -> dict:
    assert _pool is not None
    with _pool.acquire() as client:
        return {
            "tenant_id": int(client.tenant_id),
        }


@app.post("/patients", status_code=201)
def create_patient(body: CreatePatient) -> dict:
    if _pool is None:
        raise HTTPException(status_code=503, detail="pool not ready")
    try:
        with _pool.acquire() as client:
            grant = client.compliance.consent.grant(body.name, body.consent_purpose)
            return {"id": body.name, "consent_id": grant.consent_id}
    except Exception as exc:  # noqa: BLE001 — surface all errors to the client
        raise HTTPException(status_code=500, detail=str(exc))


@app.get("/patients/{patient_id}")
def get_patient(patient_id: str) -> dict:
    if _pool is None:
        raise HTTPException(status_code=503, detail="pool not ready")
    try:
        with _pool.acquire() as client:
            has_consent = client.compliance.consent.check(patient_id, "Analytics")
            return {"id": patient_id, "analytics_consent": has_consent}
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail=str(exc))
