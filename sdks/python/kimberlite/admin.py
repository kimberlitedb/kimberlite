"""Admin operations — schema, tenants, API keys, server info.

Accessed via ``client.admin`` (all methods require the Admin role):

    >>> tables = client.admin.list_tables()
    >>> info = client.admin.server_info()
    >>> print(f"Kimberlite {info.build_version} uptime={info.uptime_secs}s")
"""

from __future__ import annotations

import ctypes
import json
import re
from dataclasses import dataclass, field
from typing import List, Literal, Optional, Union

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


# -- Masking policy (v0.6.0 Tier 2 #7) -------------------------------------

MaskingStrategyKind = Literal[
    "RedactSsn",
    "RedactPhone",
    "RedactEmail",
    "RedactCreditCard",
    "RedactCustom",
    "Hash",
    "Tokenize",
    "Truncate",
    "Null",
]


@dataclass(frozen=True)
class MaskingStrategy:
    """Masking strategy descriptor for CREATE MASKING POLICY.

    The `kind` field tags the variant. `replacement` is required when
    `kind == "RedactCustom"`; `max_chars` is required when
    `kind == "Truncate"`.

    Examples:

        >>> MaskingStrategy(kind="RedactSsn")
        >>> MaskingStrategy(kind="RedactCustom", replacement="***")
        >>> MaskingStrategy(kind="Truncate", max_chars=4)
    """

    kind: MaskingStrategyKind
    replacement: Optional[str] = None
    max_chars: Optional[int] = None

    def to_ffi_json(self) -> dict:
        """Serialise to the JSON shape expected by the FFI layer."""
        out: dict = {"kind": self.kind}
        if self.kind == "RedactCustom":
            if self.replacement is None:
                raise ValueError("RedactCustom requires `replacement`")
            out["replacement"] = self.replacement
        elif self.kind == "Truncate":
            if self.max_chars is None or self.max_chars <= 0:
                raise ValueError("Truncate requires a positive `max_chars`")
            out["max_chars"] = int(self.max_chars)
        return out


@dataclass(frozen=True)
class MaskingPolicyInfo:
    name: str
    strategy: MaskingStrategy
    exempt_roles: List[str]
    default_masked: bool
    attachment_count: int


@dataclass(frozen=True)
class MaskingAttachmentInfo:
    table_name: str
    column_name: str
    policy_name: str


@dataclass(frozen=True)
class MaskingPolicyListResult:
    policies: List[MaskingPolicyInfo]
    attachments: List[MaskingAttachmentInfo] = field(default_factory=list)


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
        #: Grouped masking-policy catalogue surface.
        #: v0.6.0 Tier 2 #7. See :class:`MaskingPolicyNamespace`.
        self.masking_policy = MaskingPolicyNamespace(handle)

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


# ============================================================================
# Masking policy namespace — v0.6.0 Tier 2 #7
# ============================================================================

_IDENT_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def _validate_identifier(s: str, label: str) -> None:
    """Reject shapes that aren't ``[A-Za-z_][A-Za-z0-9_]*``.

    Prevents SQL-injection-shaped inputs reaching the DDL composer.
    """
    if not _IDENT_RE.match(s):
        raise ValueError(f"{label} '{s}' is not a valid SQL identifier")


class MaskingPolicyNamespace:
    """Grouped masking-policy catalogue operations.

    Accessed as ``client.admin.masking_policy``. Mirrors the TypeScript
    SDK's ``client.admin.maskingPolicy.*`` shape.

    Example:

        >>> client.admin.masking_policy.create(
        ...     "ssn_policy",
        ...     MaskingStrategy(kind="RedactSsn"),
        ...     ["clinician", "billing"],
        ... )
        >>> client.admin.masking_policy.attach(
        ...     "patients", "medicare_number", "ssn_policy"
        ... )
        >>> result = client.admin.masking_policy.list(include_attachments=True)
        >>> print(result.policies[0].name)
    """

    def __init__(self, handle: KmbClient) -> None:
        self._handle = handle

    def create(
        self,
        name: str,
        strategy: MaskingStrategy,
        exempt_roles: List[str],
    ) -> None:
        """Create a masking policy in this tenant's catalogue.

        Raises:
            ValueError: If ``exempt_roles`` is empty, the name is not a
                valid SQL identifier, or the strategy is malformed.
        """
        _validate_identifier(name, "policy name")
        if not exempt_roles:
            raise ValueError("exempt_roles must contain at least one role")

        err = _lib.kmb_admin_masking_policy_create(
            self._handle,
            name.encode("utf-8"),
            json.dumps(strategy.to_ffi_json()).encode("utf-8"),
            json.dumps(list(exempt_roles)).encode("utf-8"),
        )
        _check_error(err)

    def drop(self, name: str) -> None:
        """Drop a masking policy. Rejected if any column still attaches to it."""
        _validate_identifier(name, "policy name")
        err = _lib.kmb_admin_masking_policy_drop(
            self._handle, name.encode("utf-8")
        )
        _check_error(err)

    def attach(self, table: str, column: str, policy_name: str) -> None:
        """Attach a pre-existing policy to ``(table, column)``."""
        _validate_identifier(table, "table name")
        _validate_identifier(column, "column name")
        _validate_identifier(policy_name, "policy name")
        err = _lib.kmb_admin_masking_policy_attach(
            self._handle,
            table.encode("utf-8"),
            column.encode("utf-8"),
            policy_name.encode("utf-8"),
        )
        _check_error(err)

    def detach(self, table: str, column: str) -> None:
        """Detach the masking policy (if any) from ``(table, column)``."""
        _validate_identifier(table, "table name")
        _validate_identifier(column, "column name")
        err = _lib.kmb_admin_masking_policy_detach(
            self._handle,
            table.encode("utf-8"),
            column.encode("utf-8"),
        )
        _check_error(err)

    def list(self, include_attachments: bool = False) -> MaskingPolicyListResult:
        """List every masking policy in this tenant's catalogue."""
        data = _call_admin(
            _lib.kmb_admin_masking_policy_list,
            self._handle,
            ctypes.c_bool(include_attachments),
        )
        policies = [
            MaskingPolicyInfo(
                name=p["name"],
                strategy=_parse_masking_strategy(p["strategy"]),
                exempt_roles=list(p.get("exempt_roles", [])),
                default_masked=bool(p.get("default_masked", True)),
                attachment_count=int(p.get("attachment_count", 0)),
            )
            for p in data.get("policies", [])
        ]
        attachments = [
            MaskingAttachmentInfo(
                table_name=a["table_name"],
                column_name=a["column_name"],
                policy_name=a["policy_name"],
            )
            for a in data.get("attachments", [])
        ]
        return MaskingPolicyListResult(policies=policies, attachments=attachments)


def _parse_masking_strategy(raw: dict) -> MaskingStrategy:
    kind = raw["kind"]
    return MaskingStrategy(
        kind=kind,
        replacement=raw.get("replacement"),
        max_chars=raw.get("max_chars"),
    )
