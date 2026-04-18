"""Django integration — consent + audit via the Kimberlite SDK.

Wire into your project's `urls.py` as:

    from django.urls import path
    from . import views

    urlpatterns = [
        path('api/health/', views.health),
        path('api/patients/', views.create_patient),
        path('api/patients/<str:subject_id>/', views.get_patient),
    ]

The pool is initialized at module load and shared across request threads
(safe because Pool is thread-safe internally).
"""

import json

from django.http import HttpRequest, JsonResponse
from django.views.decorators.csrf import csrf_exempt
from django.views.decorators.http import require_http_methods

from kimberlite import Pool


_pool: Pool = Pool(
    address="127.0.0.1:5432",
    tenant_id=1,
    max_size=8,
)


def health(_request: HttpRequest) -> JsonResponse:
    return JsonResponse({"status": "ok"})


@csrf_exempt
@require_http_methods(["POST"])
def create_patient(request: HttpRequest) -> JsonResponse:
    payload = json.loads(request.body)
    name = payload.get("name")
    purpose = payload.get("consent_purpose")
    if not name or not purpose:
        return JsonResponse({"error": "name + consent_purpose required"}, status=400)

    try:
        with _pool.acquire() as client:
            grant = client.compliance.consent.grant(name, purpose)
            return JsonResponse(
                {"id": name, "consent_id": grant.consent_id}, status=201
            )
    except Exception as exc:  # noqa: BLE001
        return JsonResponse({"error": str(exc)}, status=500)


@require_http_methods(["GET"])
def get_patient(_request: HttpRequest, subject_id: str) -> JsonResponse:
    try:
        with _pool.acquire() as client:
            has_consent = client.compliance.consent.check(subject_id, "Analytics")
            return JsonResponse({"id": subject_id, "analytics_consent": has_consent})
    except Exception as exc:  # noqa: BLE001
        return JsonResponse({"error": str(exc)}, status=500)
