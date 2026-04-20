package com.kimberlite;

import java.util.List;
import java.util.Objects;

/**
 * Typed translation of wire-level Kimberlite errors into the
 * categories an application actually branches on (HTTP status code,
 * retry policy, alerting, etc.).
 *
 * <p>AUDIT-2026-04 S4.2 — mirrors the TypeScript / Rust / Go SDK
 * {@code DomainError} surfaces so every downstream app that talks to
 * Kimberlite from Java can do the same pattern-matching on outcomes
 * without rewriting a translator.
 *
 * <p>Translate a thrown {@link KimberliteException} via
 * {@link DomainErrors#from(Throwable)}:
 *
 * <pre>{@code
 * try {
 *     rows = client.query(...);
 * } catch (KimberliteException e) {
 *     DomainError domain = DomainErrors.from(e);
 *     switch (domain.getKind()) {
 *         case NOT_FOUND -> return 404;
 *         case FORBIDDEN -> return 403;
 *         case CONCURRENT_MODIFICATION -> retry();
 *         ...
 *     }
 * }
 * }</pre>
 */
public final class DomainError {

    /** Classification of the failure. */
    public enum Kind {
        /** Service is down / unreachable. */
        UNAVAILABLE,
        /** Named resource (stream, table, tenant, api-key) does not exist. */
        NOT_FOUND,
        /** Authentication failed or actor lacks permission. */
        FORBIDDEN,
        /** Optimistic-concurrency conflict — caller should re-read + retry. */
        CONCURRENT_MODIFICATION,
        /** Resource-level conflict (already exists, duplicate key). */
        CONFLICT,
        /** Server-enforced invariant was violated — usually a bug on our end. */
        INVARIANT_VIOLATION,
        /** Server is rate-limiting the caller. */
        RATE_LIMITED,
        /** Operation exceeded its deadline. */
        TIMEOUT,
        /** Client input failed validation (bad SQL, bad offset, etc.). */
        VALIDATION
    }

    private final Kind kind;
    private final String message;
    private final List<String> reasons;
    private final String invariantName;

    DomainError(Kind kind, String message, List<String> reasons, String invariantName) {
        this.kind = Objects.requireNonNull(kind, "kind must not be null");
        this.message = Objects.requireNonNull(message, "message must not be null");
        this.reasons = reasons != null ? List.copyOf(reasons) : List.of();
        this.invariantName = invariantName;
    }

    public Kind getKind() {
        return kind;
    }

    public String getMessage() {
        return message;
    }

    /** Non-empty for {@link Kind#CONFLICT}. */
    public List<String> getReasons() {
        return reasons;
    }

    /** Non-null for {@link Kind#INVARIANT_VIOLATION}. */
    public String getInvariantName() {
        return invariantName;
    }

    @Override
    public String toString() {
        return "DomainError[" + kind + "]: " + message;
    }
}
