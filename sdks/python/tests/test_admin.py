"""Tests for the Python admin wrapper — shape checks without a live server.

End-to-end admin operations against a running server land in the Phase 8
framework-integration example suite.
"""

from kimberlite import (
    AdminNamespace,
    ApiKeyInfo,
    ApiKeyRegisterResult,
    ApiKeyRotateResult,
    ColumnInfo,
    DescribeTable,
    IndexInfo,
    ServerInfo,
    TableInfo,
    TenantCreateResult,
    TenantDeleteResult,
    TenantInfo,
)
from kimberlite.types import TenantId


def test_admin_namespace_is_importable():
    # Cheap sanity check: AdminNamespace can be referenced at import time.
    assert AdminNamespace.__module__ == "kimberlite.admin"


def test_admin_dataclasses_are_frozen():
    info = TableInfo(name="patients", column_count=5)
    try:
        info.name = "other"  # type: ignore[misc]
    except Exception:
        pass
    else:
        raise AssertionError("TableInfo should be frozen")


def test_server_info_shape():
    info = ServerInfo(
        build_version="0.5.0",
        protocol_version=2,
        capabilities=["query", "admin.v1"],
        uptime_secs=60,
        cluster_mode="Standalone",
        tenant_count=3,
    )
    assert info.build_version == "0.5.0"
    assert info.protocol_version == 2
    assert "admin.v1" in info.capabilities
    assert info.cluster_mode == "Standalone"


def test_tenant_create_result_shape():
    info = TenantInfo(
        tenant_id=TenantId(42),
        name="acme",
        table_count=3,
        created_at_nanos=1_700_000_000_000_000_000,
    )
    result = TenantCreateResult(tenant=info, created=True)
    assert result.created is True
    assert int(result.tenant.tenant_id) == 42
    assert result.tenant.table_count == 3


def test_api_key_register_result_shape():
    info = ApiKeyInfo(
        key_id="abcd1234",
        subject="alice",
        tenant_id=TenantId(1),
        roles=["User"],
        expires_at_nanos=None,
    )
    result = ApiKeyRegisterResult(key="kmb_live_test", info=info)
    assert result.key.startswith("kmb_")
    assert result.info.subject == "alice"
    # Plaintext key is exposed exactly once in the register result; the
    # `info` side never carries it.
    assert not hasattr(result.info, "key")


def test_api_key_rotate_result_has_new_key():
    info = ApiKeyInfo(
        key_id="new12345",
        subject="alice",
        tenant_id=TenantId(1),
        roles=["User"],
        expires_at_nanos=None,
    )
    rotate = ApiKeyRotateResult(new_key="kmb_live_xyz", info=info)
    assert rotate.new_key.startswith("kmb_")
    assert rotate.info.key_id == "new12345"


def test_column_and_index_info_shape():
    col = ColumnInfo(name="id", data_type="BIGINT", nullable=False, primary_key=True)
    idx = IndexInfo(name="idx_email", columns=["email", "status"])
    assert col.primary_key is True
    assert idx.columns == ["email", "status"]


def test_describe_table_shape():
    d = DescribeTable(
        table_name="users",
        columns=[
            ColumnInfo(name="id", data_type="BIGINT", nullable=False, primary_key=True),
        ],
    )
    assert d.table_name == "users"
    assert d.columns[0].data_type == "BIGINT"


def test_tenant_delete_result_fields():
    r = TenantDeleteResult(deleted=True, tables_dropped=7)
    assert r.deleted is True
    assert r.tables_dropped == 7
