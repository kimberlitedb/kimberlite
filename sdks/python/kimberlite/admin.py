"""Admin operations — schema, tenants, API keys, server info.

Accessed via ``client.admin`` (all methods require the Admin role):

    >>> tables = client.admin.list_tables()
    >>> info = client.admin.server_info()
    >>> print(f"Kimberlite {info.build_version} uptime={info.uptime_secs}s")
"""

from __future__ import annotations

import ctypes
import json
from dataclasses import dataclass
from typing import List, Optional

from .ffi import _check_error, _lib, KmbAdminJson, KmbClient
from .types import TenantId


@dataclass(frozen=True)
class TableInfo:
    name: str
    column_count: int


@dataclass(frozen=True)
class ColumnInfo:
    name: str
    data_type: str
    nullable: bool
    primary_key: bool


@dataclass(frozen=True)
class IndexInfo:
    name: str
    columns: List[str]


@dataclass(frozen=True)
class DescribeTable:
    table_name: str
    columns: List[ColumnInfo]


@dataclass(frozen=True)
class TenantInfo:
    tenant_id: TenantId
    name: Optional[str]
    table_count: int
    created_at_nanos: Optional[int]


@dataclass(frozen=True)
class TenantCreateResult:
    tenant: TenantInfo
    created: bool


@dataclass(frozen=True)
class TenantDeleteResult:
    deleted: bool
    tables_dropped: int


@dataclass(frozen=True)
class ApiKeyInfo:
    key_id: str
    subject: str
    tenant_id: TenantId
    roles: List[str]
    expires_at_nanos: Optional[int]


@dataclass(frozen=True)
class ApiKeyRegisterResult:
    """`key` is the plaintext — returned exactly once. Persist immediately."""

    key: str
    info: ApiKeyInfo


@dataclass(frozen=True)
class ApiKeyRotateResult:
    new_key: str
    info: ApiKeyInfo


@dataclass(frozen=True)
class ServerInfo:
    build_version: str
    protocol_version: int
    capabilities: List[str]
    uptime_secs: int
    cluster_mode: str  # "Standalone" | "Clustered"
    tenant_count: int


def _call_admin(native_fn, *args) -> dict:
    """Invoke an admin FFI function that writes a JSON blob and return parsed dict."""
    result = KmbAdminJson()
    err = native_fn(*args, ctypes.byref(result))
    _check_error(err)
    try:
        raw = ctypes.string_at(result.json).decode("utf-8") if result.json else "{}"
        return json.loads(raw)
    finally:
        _lib.kmb_admin_json_free(ctypes.byref(result))


class AdminNamespace:
    """Admin namespace accessible as ``client.admin``."""

    def __init__(self, handle: KmbClient) -> None:
        self._handle = handle

    # ----- Schema -----

    def list_tables(self) -> List[TableInfo]:
        data = _call_admin(_lib.kmb_admin_list_tables, self._handle)
        return [
            TableInfo(name=t["name"], column_count=int(t["column_count"]))
            for t in data.get("tables", [])
        ]

    def describe_table(self, table_name: str) -> DescribeTable:
        data = _call_admin(
            _lib.kmb_admin_describe_table,
            self._handle,
            table_name.encode("utf-8"),
        )
        columns = [
            ColumnInfo(
                name=c["name"],
                data_type=c["data_type"],
                nullable=bool(c["nullable"]),
                primary_key=bool(c["primary_key"]),
            )
            for c in data.get("columns", [])
        ]
        return DescribeTable(table_name=data.get("table_name", table_name), columns=columns)

    def list_indexes(self, table_name: str) -> List[IndexInfo]:
        data = _call_admin(
            _lib.kmb_admin_list_indexes,
            self._handle,
            table_name.encode("utf-8"),
        )
        return [
            IndexInfo(name=i["name"], columns=list(i.get("columns", [])))
            for i in data.get("indexes", [])
        ]

    # ----- Tenants -----

    def create_tenant(
        self, tenant_id: TenantId, name: Optional[str] = None
    ) -> TenantCreateResult:
        data = _call_admin(
            _lib.kmb_admin_tenant_create,
            self._handle,
            int(tenant_id),
            name.encode("utf-8") if name else None,
        )
        return TenantCreateResult(
            tenant=_parse_tenant_info(data["tenant"]),
            created=bool(data["created"]),
        )

    def list_tenants(self) -> List[TenantInfo]:
        data = _call_admin(_lib.kmb_admin_tenant_list, self._handle)
        return [_parse_tenant_info(t) for t in data.get("tenants", [])]

    def delete_tenant(self, tenant_id: TenantId) -> TenantDeleteResult:
        data = _call_admin(_lib.kmb_admin_tenant_delete, self._handle, int(tenant_id))
        return TenantDeleteResult(
            deleted=bool(data["deleted"]),
            tables_dropped=int(data["tables_dropped"]),
        )

    def get_tenant(self, tenant_id: TenantId) -> TenantInfo:
        data = _call_admin(_lib.kmb_admin_tenant_get, self._handle, int(tenant_id))
        return _parse_tenant_info(data["tenant"])

    # ----- API keys -----

    def issue_api_key(
        self,
        subject: str,
        tenant_id: TenantId,
        roles: List[str],
        expires_at_nanos: Optional[int] = None,
    ) -> ApiKeyRegisterResult:
        """Issue a new API key. Plaintext key is returned exactly once."""
        data = _call_admin(
            _lib.kmb_admin_api_key_register,
            self._handle,
            subject.encode("utf-8"),
            int(tenant_id),
            json.dumps(roles).encode("utf-8"),
            int(expires_at_nanos) if expires_at_nanos is not None else 0,
        )
        return ApiKeyRegisterResult(
            key=data["key"],
            info=_parse_api_key_info(data["info"]),
        )

    def revoke_api_key(self, key: str) -> bool:
        data = _call_admin(
            _lib.kmb_admin_api_key_revoke,
            self._handle,
            key.encode("utf-8"),
        )
        return bool(data.get("revoked", False))

    def list_api_keys(self, tenant_id: Optional[TenantId] = None) -> List[ApiKeyInfo]:
        data = _call_admin(
            _lib.kmb_admin_api_key_list,
            self._handle,
            int(tenant_id) if tenant_id is not None else 0,
        )
        return [_parse_api_key_info(k) for k in data.get("keys", [])]

    def rotate_api_key(self, old_key: str) -> ApiKeyRotateResult:
        data = _call_admin(
            _lib.kmb_admin_api_key_rotate,
            self._handle,
            old_key.encode("utf-8"),
        )
        return ApiKeyRotateResult(
            new_key=data["new_key"],
            info=_parse_api_key_info(data["info"]),
        )

    # ----- Server info -----

    def server_info(self) -> ServerInfo:
        data = _call_admin(_lib.kmb_admin_server_info, self._handle)
        return ServerInfo(
            build_version=data["build_version"],
            protocol_version=int(data["protocol_version"]),
            capabilities=list(data.get("capabilities", [])),
            uptime_secs=int(data["uptime_secs"]),
            cluster_mode=data["cluster_mode"],
            tenant_count=int(data["tenant_count"]),
        )


def _parse_tenant_info(raw: dict) -> TenantInfo:
    return TenantInfo(
        tenant_id=TenantId(int(raw["tenant_id"])),
        name=raw.get("name"),
        table_count=int(raw.get("table_count", 0)),
        created_at_nanos=raw.get("created_at_nanos"),
    )


def _parse_api_key_info(raw: dict) -> ApiKeyInfo:
    return ApiKeyInfo(
        key_id=raw["key_id"],
        subject=raw["subject"],
        tenant_id=TenantId(int(raw["tenant_id"])),
        roles=list(raw.get("roles", [])),
        expires_at_nanos=raw.get("expires_at_nanos"),
    )
