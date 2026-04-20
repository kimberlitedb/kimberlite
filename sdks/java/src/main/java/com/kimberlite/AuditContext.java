package com.kimberlite;

import java.util.Objects;

/**
 * Caller attribution carried on every Kimberlite client call.
 *
 * <p>{@code actor} and {@code reason} are mandatory in regulated-industry
 * apps (HIPAA minimum-necessary, GDPR purpose limitation, FedRAMP
 * audit-trail completeness). {@code correlationId} and
 * {@code idempotencyKey} are optional.
 *
 * <p>AUDIT-2026-04 S3.9 — the SDK threads this onto the outgoing wire
 * {@code Request.audit} so the server's {@code ComplianceAuditLog}
 * records the operator's identity on every mutation (consent grant,
 * erasure request, break-glass query, etc.).
 *
 * <p>Typical pattern — wrap each request handler with an
 * {@link AuditScope} so downstream SDK calls pick up the context
 * transparently:
 *
 * <pre>{@code
 * try (AuditScope scope = AuditContext.newScope(
 *         new AuditContext(userId, "patient-chart-view")
 *             .withCorrelationId(req.getHeader("X-Request-Id")))) {
 *     QueryResult rows = client.query("SELECT * FROM patients WHERE id = ?", id);
 *     ...
 * }
 * }</pre>
 */
public final class AuditContext {

    /** Principal identifier — opaque to the server, typically a user UUID or email. */
    private final String actor;
    /** Free-form "why" for the access — break-glass / minimum-necessary justification. */
    private final String reason;
    /** Distributed-tracing correlation id (e.g. HTTP {@code X-Request-Id}). */
    private final String correlationId;
    /** Caller-chosen idempotency key for retry de-duplication. */
    private final String idempotencyKey;

    public AuditContext(String actor, String reason) {
        this(actor, reason, null, null);
    }

    private AuditContext(
            String actor,
            String reason,
            String correlationId,
            String idempotencyKey) {
        this.actor = Objects.requireNonNull(actor, "actor must not be null");
        this.reason = Objects.requireNonNull(reason, "reason must not be null");
        this.correlationId = correlationId;
        this.idempotencyKey = idempotencyKey;
    }

    public String getActor() {
        return actor;
    }

    public String getReason() {
        return reason;
    }

    public String getCorrelationId() {
        return correlationId;
    }

    public String getIdempotencyKey() {
        return idempotencyKey;
    }

    /** Returns a copy of this context with {@code correlationId} set. */
    public AuditContext withCorrelationId(String correlationId) {
        return new AuditContext(actor, reason, correlationId, idempotencyKey);
    }

    /** Returns a copy of this context with {@code idempotencyKey} set. */
    public AuditContext withIdempotencyKey(String idempotencyKey) {
        return new AuditContext(actor, reason, correlationId, idempotencyKey);
    }

    // --- Thread-local propagation ---

    private static final ThreadLocal<AuditContext> CURRENT = new ThreadLocal<>();

    /**
     * Install {@code ctx} as the active audit context on the current
     * thread. The returned {@link AuditScope} restores the previous
     * context when closed — use with try-with-resources.
     */
    public static AuditScope newScope(AuditContext ctx) {
        Objects.requireNonNull(ctx, "ctx must not be null");
        AuditContext previous = CURRENT.get();
        CURRENT.set(ctx);
        return new AuditScope(previous);
    }

    /**
     * Returns the currently-active audit context, or {@code null} if
     * none is set.
     */
    public static AuditContext current() {
        return CURRENT.get();
    }

    /**
     * Returns the currently-active audit context, throwing {@link IllegalStateException}
     * if none is set. Use at call sites that refuse to run without
     * attribution (break-glass queries, PHI exports, compliance reports).
     */
    public static AuditContext require() {
        AuditContext ctx = CURRENT.get();
        if (ctx == null) {
            throw new IllegalStateException(
                "AuditContext.require(): no audit context active — wrap the call in "
                    + "`try (var s = AuditContext.newScope(...)) { ... }`"
            );
        }
        return ctx;
    }

    /**
     * RAII guard that restores the previous audit context on close.
     * Returned by {@link AuditContext#newScope(AuditContext)}.
     */
    public static final class AuditScope implements AutoCloseable {
        private final AuditContext previous;
        private boolean closed;

        AuditScope(AuditContext previous) {
            this.previous = previous;
        }

        @Override
        public void close() {
            if (closed) {
                return;
            }
            closed = true;
            if (previous != null) {
                CURRENT.set(previous);
            } else {
                CURRENT.remove();
            }
        }
    }
}
