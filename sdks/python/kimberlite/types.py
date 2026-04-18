"""Type definitions for Kimberlite Python SDK."""

from enum import IntEnum
from typing import NewType

# Type aliases for clarity
StreamId = NewType("StreamId", int)
Offset = NewType("Offset", int)
TenantId = NewType("TenantId", int)


class DataClass(IntEnum):
    """Data classification for streams.

    Values mirror the 8 variants of `kimberlite_types::DataClass` and the
    ``KmbDataClass`` FFI enum.

    Attributes:
        PHI: Protected Health Information (HIPAA-regulated)
        NON_PHI: Non-PHI data (alias for PUBLIC — preserved for compatibility)
        DEIDENTIFIED: De-identified data (HIPAA Safe Harbor)
        PII: Personally Identifiable Information (GDPR Art. 4)
        SENSITIVE: GDPR Article 9 special-category data
        PCI: Payment Card Industry data (PCI DSS)
        FINANCIAL: Financial records (SOX / GLBA)
        CONFIDENTIAL: Internal / confidential business data
        PUBLIC: Publicly available data
    """

    PHI = 0
    NON_PHI = 1
    DEIDENTIFIED = 2
    PII = 3
    SENSITIVE = 4
    PCI = 5
    FINANCIAL = 6
    CONFIDENTIAL = 7
    PUBLIC = 8


class Placement(IntEnum):
    """Geographic placement policy for a stream.

    Matches the ``KmbPlacement`` FFI enum.  ``CUSTOM`` requires passing a
    ``custom_region`` argument to ``Client.create_stream``; the other values
    are self-describing.

    Attributes:
        GLOBAL: Global replication across all regions (default)
        US_EAST_1: US East (N. Virginia) - us-east-1
        AP_SOUTHEAST_2: Asia Pacific (Sydney) - ap-southeast-2
        CUSTOM: Custom region identifier (pass ``custom_region`` argument)
    """

    GLOBAL = 0
    US_EAST_1 = 1
    AP_SOUTHEAST_2 = 2
    CUSTOM = 3
