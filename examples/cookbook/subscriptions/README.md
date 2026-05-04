# Cookbook: Real-time subscriptions

What this teaches:

- `client.subscribe(streamId, { startOffset })` returns an
  `AsyncIterable<SubscriptionEvent>` — drive it with `for await`.
- Subscriptions are **push, not poll**. Notebar's pre-v0.7.0
  integration repeatedly polled `client.read()` because nothing in
  `examples/` made the push surface visible.
- Backpressure is built in via credit-based flow control. The SDK
  auto-refills credits when the in-flight count drops below the
  `lowWater` mark; you can also `subscription.grantCredits(n)`
  manually.
- `subscription.unsubscribe()` is idempotent and safe to call from
  cleanup hooks (`React useEffect` return, signal handlers).

Prerequisites:

```bash
just kmb-server-dev   # runs on 5432
```

Run:

```bash
cd typescript && pnpm install && pnpm tsx main.ts
# or
cd python && python main.py
```

Expected stdout (last line):

```
KMB_COOKBOOK_OK
```

## Why this matters

Notebar Phase 4 (real-time appointment updates in the practitioner
dashboard) was originally built on polling because the team didn't
realise the SDK already had a streaming primitive. The streaming
path is ~10× lower latency and ~50× lower load on the server.

## Related docs

- [`docs/coding/recipes/subscriptions.md`](../../../docs/coding/recipes/subscriptions.md)
- [`sdks/typescript/src/subscription.ts`](../../../sdks/typescript/src/subscription.ts) — implementation reference
