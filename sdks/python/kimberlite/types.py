"""Type definitions for Kimberlite Python SDK."""

from enum import IntEnum
from typing import NewType

# Type aliases for clarity
StreamId = NewType("StreamId", int)
Offset = NewType("Offset", int)
TenantId = NewType("TenantId", int)


class DataClass(IntEnum):
    """Data classification for streams.

    Attributes:
        PHI: Protected Health Information (HIPAA-regulated)
        NON_PHI: Non-PHI data
        DEIDENTIFIED: De-identified data
    """

    PHI = 0
    NON_PHI = 1
    DEIDENTIFIED = 2
