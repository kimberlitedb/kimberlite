package com.kimberlite;

import java.util.List;

/**
 * Static factory for translating {@link KimberliteException} (or any
 * {@link Throwable}) into a {@link DomainError}. Non-Kimberlite
 * exceptions fall through to {@link DomainError.Kind#UNAVAILABLE} with
 * the raw message — never opaque, never reveals stack frames.
 */
public final class DomainErrors {

    private DomainErrors() {
        // Utility class.
    }

    /**
     * Translate a thrown error into a {@link DomainError}. Returns
     * {@code null} if {@code t} is {@code null}.
     */
    public static DomainError from(Throwable t) {
        if (t == null) {
            return null;
        }

        if (t instanceof KimberliteException ke) {
            return fromException(ke);
        }

        String msg = t.getMessage();
        return new DomainError(
            DomainError.Kind.UNAVAILABLE,
            msg != null ? msg : t.toString(),
            null,
            null
        );
    }

    private static DomainError fromException(KimberliteException ke) {
        String msg = ke.getMessage() != null ? ke.getMessage() : ke.getCode().name();
        switch (ke.getCode()) {
            case CONNECTION_FAILED:
                return new DomainError(DomainError.Kind.UNAVAILABLE, msg, null, null);
            case QUERY_FAILED, INVALID_ARGUMENT:
                return new DomainError(DomainError.Kind.VALIDATION, msg, null, null);
            case UNAUTHORIZED:
                return new DomainError(DomainError.Kind.FORBIDDEN, msg, null, null);
            case APPEND_FAILED:
                // Append failures are usually OffsetMismatch server-side,
                // which semantically maps to concurrent-modification.
                return new DomainError(
                    DomainError.Kind.CONCURRENT_MODIFICATION,
                    msg,
                    null,
                    null
                );
            case SERVER_BUSY:
                return new DomainError(
                    DomainError.Kind.RATE_LIMITED,
                    msg,
                    List.of(msg),
                    null
                );
            default:
                return new DomainError(DomainError.Kind.UNAVAILABLE, msg, null, null);
        }
    }
}
