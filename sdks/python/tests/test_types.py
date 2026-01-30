"""Tests for type definitions."""

import pytest
from kimberlite.types import DataClass, StreamId, Offset


def test_data_class_values():
    """Test DataClass enum values."""
    assert DataClass.PHI == 0
    assert DataClass.NON_PHI == 1
    assert DataClass.DEIDENTIFIED == 2


def test_stream_id_type():
    """Test StreamId type alias."""
    stream_id = StreamId(42)
    assert int(stream_id) == 42


def test_offset_type():
    """Test Offset type alias."""
    offset = Offset(100)
    assert int(offset) == 100
