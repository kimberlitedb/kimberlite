"""Integration tests against live kmb-server.

These tests require a running kmb-server instance.
Run with: pytest tests/test_integration.py
"""

import pytest
from kimberlite import Client, DataClass, StreamNotFoundError


@pytest.fixture
def client():
    """Connect to test server."""
    try:
        client = Client.connect(
            addresses=["localhost:5432"],
            tenant_id=1,
            auth_token="test-token",
        )
        yield client
        client.disconnect()
    except Exception as e:
        pytest.skip(f"Server not available: {e}")


def test_create_stream(client):
    """Test creating a stream."""
    stream_id = client.create_stream("test_stream", DataClass.NON_PHI)
    assert stream_id > 0


def test_append_and_read(client):
    """Test appending and reading events."""
    # Create stream
    stream_id = client.create_stream("append_test", DataClass.NON_PHI)

    # Append events
    events = [
        b"event1",
        b"event2",
        b"event3",
    ]
    first_offset = client.append(stream_id, events)
    assert first_offset >= 0

    # Read events back
    read_events = client.read(stream_id, from_offset=first_offset, max_bytes=1024)
    assert len(read_events) == 3
    assert read_events[0].data == b"event1"
    assert read_events[1].data == b"event2"
    assert read_events[2].data == b"event3"


def test_context_manager(client):
    """Test context manager properly disconnects."""
    with Client.connect(
        addresses=["localhost:5432"],
        tenant_id=1,
        auth_token="test-token",
    ) as ctx_client:
        stream_id = ctx_client.create_stream("ctx_test", DataClass.NON_PHI)
        assert stream_id > 0

    # After context exit, client should be closed
    assert ctx_client._closed


def test_stream_not_found(client):
    """Test error handling for non-existent stream."""
    with pytest.raises(StreamNotFoundError):
        client.read(9999999, from_offset=0, max_bytes=1024)


def test_empty_append_fails(client):
    """Test that appending empty event list fails."""
    stream_id = client.create_stream("empty_test", DataClass.NON_PHI)

    with pytest.raises(ValueError, match="Cannot append empty event list"):
        client.append(stream_id, [])
