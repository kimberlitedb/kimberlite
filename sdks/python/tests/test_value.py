"""Tests for value types."""

from datetime import datetime, timezone
import pytest

from kimberlite.value import Value, ValueType


class TestValueNull:
    """Tests for NULL values."""

    def test_create_null(self):
        val = Value.null()
        assert val.type == ValueType.NULL
        assert val.data is None
        assert val.is_null()

    def test_null_repr(self):
        val = Value.null()
        assert repr(val) == "Value.null()"

    def test_null_str(self):
        val = Value.null()
        assert str(val) == "NULL"


class TestValueBigInt:
    """Tests for BIGINT values."""

    def test_create_bigint(self):
        val = Value.bigint(42)
        assert val.type == ValueType.BIGINT
        assert val.data == 42
        assert not val.is_null()

    def test_bigint_negative(self):
        val = Value.bigint(-100)
        assert val.data == -100

    def test_bigint_zero(self):
        val = Value.bigint(0)
        assert val.data == 0

    def test_bigint_large_positive(self):
        val = Value.bigint(2**63 - 1)  # Max i64
        assert val.data == 2**63 - 1

    def test_bigint_large_negative(self):
        val = Value.bigint(-(2**63))  # Min i64
        assert val.data == -(2**63)

    def test_bigint_out_of_range_positive(self):
        with pytest.raises(ValueError, match="out of range"):
            Value.bigint(2**63)

    def test_bigint_out_of_range_negative(self):
        with pytest.raises(ValueError, match="out of range"):
            Value.bigint(-(2**63) - 1)

    def test_bigint_type_error(self):
        with pytest.raises(TypeError, match="expected int"):
            Value.bigint("42")

    def test_bigint_repr(self):
        val = Value.bigint(42)
        assert repr(val) == "Value(BIGINT, 42)"

    def test_bigint_str(self):
        val = Value.bigint(42)
        assert str(val) == "42"


class TestValueText:
    """Tests for TEXT values."""

    def test_create_text(self):
        val = Value.text("hello")
        assert val.type == ValueType.TEXT
        assert val.data == "hello"
        assert not val.is_null()

    def test_text_empty(self):
        val = Value.text("")
        assert val.data == ""

    def test_text_unicode(self):
        val = Value.text("Hello, 世界! 🌍")
        assert val.data == "Hello, 世界! 🌍"

    def test_text_type_error(self):
        with pytest.raises(TypeError, match="expected str"):
            Value.text(42)

    def test_text_repr(self):
        val = Value.text("world")
        assert repr(val) == "Value(TEXT, 'world')"

    def test_text_str(self):
        val = Value.text("world")
        assert str(val) == "world"


class TestValueBoolean:
    """Tests for BOOLEAN values."""

    def test_create_boolean_true(self):
        val = Value.boolean(True)
        assert val.type == ValueType.BOOLEAN
        assert val.data is True
        assert not val.is_null()

    def test_create_boolean_false(self):
        val = Value.boolean(False)
        assert val.type == ValueType.BOOLEAN
        assert val.data is False

    def test_boolean_type_error(self):
        with pytest.raises(TypeError, match="expected bool"):
            Value.boolean(1)

    def test_boolean_repr(self):
        val = Value.boolean(True)
        assert repr(val) == "Value(BOOLEAN, True)"

    def test_boolean_str(self):
        val = Value.boolean(False)
        assert str(val) == "False"


class TestValueTimestamp:
    """Tests for TIMESTAMP values."""

    def test_create_timestamp(self):
        val = Value.timestamp(1234567890)
        assert val.type == ValueType.TIMESTAMP
        assert val.data == 1234567890
        assert not val.is_null()

    def test_timestamp_zero(self):
        val = Value.timestamp(0)
        assert val.data == 0

    def test_timestamp_negative(self):
        val = Value.timestamp(-1000)
        assert val.data == -1000

    def test_timestamp_type_error(self):
        with pytest.raises(TypeError, match="expected int"):
            Value.timestamp("1234567890")

    def test_timestamp_repr(self):
        val = Value.timestamp(1234567890)
        assert repr(val) == "Value(TIMESTAMP, 1234567890)"

    def test_timestamp_str(self):
        val = Value.timestamp(1234567890)
        assert str(val) == "1234567890"


class TestValueDatetimeConversion:
    """Tests for datetime conversion."""

    def test_from_datetime(self):
        dt = datetime(2024, 1, 1, 12, 0, 0)
        val = Value.from_datetime(dt)
        assert val.type == ValueType.TIMESTAMP
        assert isinstance(val.data, int)

    def test_from_datetime_type_error(self):
        with pytest.raises(TypeError, match="expected datetime"):
            Value.from_datetime("2024-01-01")

    def test_to_datetime(self):
        # 2021-01-01 00:00:00 UTC in nanoseconds
        nanos = 1609459200_000_000_000
        val = Value.timestamp(nanos)
        dt = val.to_datetime()
        assert isinstance(dt, datetime)
        # Check year, month, day (don't check exact timezone due to platform differences)
        assert dt.year == 2021
        assert dt.month == 1
        assert dt.day == 1

    def test_to_datetime_roundtrip(self):
        original_dt = datetime(2024, 6, 15, 14, 30, 45)
        val = Value.from_datetime(original_dt)
        reconstructed_dt = val.to_datetime()

        # Allow small difference due to nanosecond precision
        diff = abs((original_dt - reconstructed_dt).total_seconds())
        assert diff < 0.001  # Less than 1 millisecond difference

    def test_to_datetime_on_non_timestamp(self):
        val = Value.bigint(42)
        assert val.to_datetime() is None


class TestValueEquality:
    """Tests for value equality."""

    def test_null_equality(self):
        assert Value.null() == Value.null()

    def test_bigint_equality(self):
        assert Value.bigint(42) == Value.bigint(42)
        assert Value.bigint(42) != Value.bigint(43)

    def test_text_equality(self):
        assert Value.text("hello") == Value.text("hello")
        assert Value.text("hello") != Value.text("world")

    def test_boolean_equality(self):
        assert Value.boolean(True) == Value.boolean(True)
        assert Value.boolean(True) != Value.boolean(False)

    def test_timestamp_equality(self):
        assert Value.timestamp(1234) == Value.timestamp(1234)
        assert Value.timestamp(1234) != Value.timestamp(5678)

    def test_different_types_not_equal(self):
        assert Value.bigint(42) != Value.text("42")
        assert Value.null() != Value.bigint(0)
        assert Value.boolean(True) != Value.bigint(1)


class TestValueHashing:
    """Tests for value hashing."""

    def test_null_hash(self):
        assert hash(Value.null()) == hash(Value.null())

    def test_bigint_hash(self):
        assert hash(Value.bigint(42)) == hash(Value.bigint(42))

    def test_text_hash(self):
        assert hash(Value.text("hello")) == hash(Value.text("hello"))

    def test_boolean_hash(self):
        assert hash(Value.boolean(True)) == hash(Value.boolean(True))

    def test_timestamp_hash(self):
        assert hash(Value.timestamp(1234)) == hash(Value.timestamp(1234))

    def test_can_use_in_set(self):
        values = {Value.bigint(1), Value.text("a"), Value.null()}
        assert Value.bigint(1) in values
        assert Value.bigint(2) not in values

    def test_can_use_as_dict_key(self):
        mapping = {
            Value.bigint(1): "one",
            Value.text("a"): "alpha",
            Value.null(): "none",
        }
        assert mapping[Value.bigint(1)] == "one"
        assert mapping[Value.null()] == "none"
