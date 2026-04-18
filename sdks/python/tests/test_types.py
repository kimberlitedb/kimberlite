"""Tests for type definitions and parity surface."""

import pytest

from kimberlite import DataClass, Placement, ExecuteResult
from kimberlite.types import StreamId, Offset


def test_data_class_legacy_values():
    """The original three FFI values remain at 0/1/2 for binary compat."""
    assert DataClass.PHI == 0
    assert DataClass.NON_PHI == 1
    assert DataClass.DEIDENTIFIED == 2


def test_data_class_full_coverage():
    """Every DataClass variant has a stable integer value matching the FFI enum."""
    assert DataClass.PII == 3
    assert DataClass.SENSITIVE == 4
    assert DataClass.PCI == 5
    assert DataClass.FINANCIAL == 6
    assert DataClass.CONFIDENTIAL == 7
    assert DataClass.PUBLIC == 8


def test_placement_enum_values():
    """Placement variants match the FFI enum (KmbPlacement)."""
    assert Placement.GLOBAL == 0
    assert Placement.US_EAST_1 == 1
    assert Placement.AP_SOUTHEAST_2 == 2
    assert Placement.CUSTOM == 3


def test_execute_result_is_frozen_dataclass():
    """ExecuteResult exposes rows_affected + log_offset and is immutable."""
    r = ExecuteResult(rows_affected=3, log_offset=1024)
    assert r.rows_affected == 3
    assert r.log_offset == 1024
    with pytest.raises(Exception):
        r.rows_affected = 99  # type: ignore[misc]


def test_stream_id_type():
    """Test StreamId type alias."""
    stream_id = StreamId(42)
    assert int(stream_id) == 42


def test_offset_type():
    """Test Offset type alias."""
    offset = Offset(100)
    assert int(offset) == 100
