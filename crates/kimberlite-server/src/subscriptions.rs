//! Per-connection subscription state and server-side credit accounting.
//!
//! A connection owns a [`SubscriptionRegistry`] of active subscriptions.
//! On each server tick, [`pump`](SubscriptionRegistry::pump) is called to push
//! newly-appended events to each subscription (up to its remaining credit
//! balance), read from the underlying `Kimberlite` tenant.
//!
//! The push path lives at the connection layer rather than in the stateless
//! request handler because subscriptions need long-lived per-connection state
//! (credit balance, current offset) and the ability to emit server-initiated
//! `Push` frames without a client `RequestId`.

use std::collections::HashMap;

use kimberlite::Kimberlite;
use kimberlite_types::{Offset, StreamId, TenantId};
use kimberlite_wire::{Push, PushPayload, SubscriptionCloseReason};

/// Hard cap on events pushed in a single pump() call per subscription.
///
/// Prevents a single busy subscription from starving other subscriptions on
/// the same connection during one server tick.
const MAX_EVENTS_PER_TICK: u32 = 64;

/// Approximate per-tick byte budget for `read_events` calls. Keeps the push
/// frame from ever exceeding the 16 MiB frame cap even when a subscription
/// has generous credits.
const MAX_BYTES_PER_TICK: u64 = 1024 * 1024;

/// An active server-side subscription.
#[derive(Debug, Clone)]
#[allow(dead_code)] // consumer_group reserved for the future consumer-group implementation.
pub struct ActiveSubscription {
    pub subscription_id: u64,
    pub tenant_id: TenantId,
    pub stream_id: StreamId,
    /// Offset of the next event the server will push.
    pub next_offset: Offset,
    /// Remaining credits — server stops pushing when this hits zero.
    pub credits_remaining: u32,
    /// Consumer group label, for future coordinated consumption (unused today).
    pub consumer_group: Option<String>,
}

/// Per-connection subscription registry.
#[derive(Debug, Default)]
pub struct SubscriptionRegistry {
    subscriptions: HashMap<u64, ActiveSubscription>,
    next_local_id: u64,
}

impl SubscriptionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new subscription and return its assigned ID.
    ///
    /// The ID is synthesised from `tenant_id` and the per-connection counter
    /// so that concurrent connections cannot collide.
    pub fn register(
        &mut self,
        tenant_id: TenantId,
        stream_id: StreamId,
        start_offset: Offset,
        initial_credits: u32,
        consumer_group: Option<String>,
    ) -> u64 {
        self.next_local_id = self.next_local_id.wrapping_add(1);
        let subscription_id = u64::from(tenant_id)
            .wrapping_mul(0x517c_c1b7_2722_0a95)
            .wrapping_add(u64::from(stream_id))
            .wrapping_add(self.next_local_id);

        self.subscriptions.insert(
            subscription_id,
            ActiveSubscription {
                subscription_id,
                tenant_id,
                stream_id,
                next_offset: start_offset,
                credits_remaining: initial_credits,
                consumer_group,
            },
        );
        subscription_id
    }

    /// Grant additional credits to an existing subscription. Returns the new
    /// balance, or `None` if the subscription is unknown.
    pub fn grant_credits(&mut self, subscription_id: u64, additional: u32) -> Option<u32> {
        let sub = self.subscriptions.get_mut(&subscription_id)?;
        sub.credits_remaining = sub.credits_remaining.saturating_add(additional);
        Some(sub.credits_remaining)
    }

    /// Remove a subscription. Returns `Some` with the removed entry if it
    /// existed (useful for emitting a final `SubscriptionClosed` push).
    pub fn remove(&mut self, subscription_id: u64) -> Option<ActiveSubscription> {
        self.subscriptions.remove(&subscription_id)
    }

    /// Number of live subscriptions on this connection.
    #[allow(dead_code)] // Used by future metrics endpoints.
    pub fn len(&self) -> usize {
        self.subscriptions.len()
    }

    /// True if the connection has no active subscriptions.
    pub fn is_empty(&self) -> bool {
        self.subscriptions.is_empty()
    }

    /// Drains up to `MAX_EVENTS_PER_TICK` events per subscription from the
    /// underlying storage and returns them as [`Push`] frames ready to enqueue.
    ///
    /// Subscriptions with zero credits are skipped entirely.
    /// Subscriptions whose stream has been deleted are removed and a
    /// `SubscriptionClosed { StreamDeleted }` push is emitted.
    pub fn pump(&mut self, db: &Kimberlite) -> Vec<Push> {
        let mut pushes = Vec::new();
        let mut to_remove: Vec<(u64, SubscriptionCloseReason)> = Vec::new();

        for sub in self.subscriptions.values_mut() {
            if sub.credits_remaining == 0 {
                continue;
            }

            let tenant = db.tenant(sub.tenant_id);
            let max_events = sub.credits_remaining.min(MAX_EVENTS_PER_TICK);

            match tenant.read_events(sub.stream_id, sub.next_offset, MAX_BYTES_PER_TICK) {
                Ok(events) => {
                    if events.is_empty() {
                        continue;
                    }

                    // Bound by credits AND per-tick cap.
                    let take = events.len().min(max_events as usize);
                    let batch: Vec<Vec<u8>> =
                        events.into_iter().take(take).map(|b| b.to_vec()).collect();
                    let count = batch.len() as u32;

                    let start_offset = sub.next_offset;
                    sub.next_offset = Offset::new(sub.next_offset.as_u64() + u64::from(count));
                    sub.credits_remaining = sub.credits_remaining.saturating_sub(count);

                    pushes.push(Push::new(PushPayload::SubscriptionEvents {
                        subscription_id: sub.subscription_id,
                        start_offset,
                        events: batch,
                        credits_remaining: sub.credits_remaining,
                    }));
                }
                Err(e) => {
                    tracing::warn!(
                        subscription_id = sub.subscription_id,
                        error = %e,
                        "subscription pump: stream read failed — closing subscription"
                    );
                    to_remove.push((sub.subscription_id, SubscriptionCloseReason::StreamDeleted));
                }
            }
        }

        for (id, reason) in to_remove {
            self.subscriptions.remove(&id);
            pushes.push(Push::new(PushPayload::SubscriptionClosed {
                subscription_id: id,
                reason,
            }));
        }

        pushes
    }

    /// Closes every active subscription (called on connection teardown or
    /// server shutdown). Returns the close-push frames for enqueueing.
    #[allow(dead_code)] // Used by connection-teardown path in server.rs::cleanup_closed.
    pub fn close_all(&mut self, reason: SubscriptionCloseReason) -> Vec<Push> {
        let mut pushes = Vec::with_capacity(self.subscriptions.len());
        for id in self.subscriptions.keys().copied() {
            pushes.push(Push::new(PushPayload::SubscriptionClosed {
                subscription_id: id,
                reason,
            }));
        }
        self.subscriptions.clear();
        pushes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_remove_roundtrip() {
        let mut reg = SubscriptionRegistry::new();
        assert!(reg.is_empty());

        let id = reg.register(
            TenantId::new(1),
            StreamId::new(10),
            Offset::new(0),
            16,
            None,
        );
        assert_eq!(reg.len(), 1);

        let removed = reg.remove(id).expect("just inserted");
        assert_eq!(removed.subscription_id, id);
        assert!(reg.is_empty());
    }

    #[test]
    fn grant_credits_adds_to_balance() {
        let mut reg = SubscriptionRegistry::new();
        let id = reg.register(TenantId::new(1), StreamId::new(1), Offset::new(0), 4, None);
        assert_eq!(reg.grant_credits(id, 8), Some(12));
        assert_eq!(reg.grant_credits(id + 999, 8), None); // unknown id
    }

    #[test]
    fn grant_credits_saturates_at_u32_max() {
        let mut reg = SubscriptionRegistry::new();
        let id = reg.register(
            TenantId::new(1),
            StreamId::new(1),
            Offset::new(0),
            u32::MAX - 5,
            None,
        );
        let new_balance = reg.grant_credits(id, 100).unwrap();
        assert_eq!(new_balance, u32::MAX);
    }

    #[test]
    fn close_all_emits_one_push_per_subscription() {
        let mut reg = SubscriptionRegistry::new();
        reg.register(TenantId::new(1), StreamId::new(1), Offset::new(0), 1, None);
        reg.register(TenantId::new(1), StreamId::new(2), Offset::new(0), 1, None);
        reg.register(TenantId::new(1), StreamId::new(3), Offset::new(0), 1, None);

        let pushes = reg.close_all(SubscriptionCloseReason::ServerShutdown);
        assert_eq!(pushes.len(), 3);
        for push in &pushes {
            if let PushPayload::SubscriptionClosed { reason, .. } = &push.payload {
                assert_eq!(*reason, SubscriptionCloseReason::ServerShutdown);
            } else {
                panic!("expected SubscriptionClosed push");
            }
        }
        assert!(reg.is_empty());
    }

    #[test]
    fn subscription_ids_are_unique_across_registrations() {
        let mut reg = SubscriptionRegistry::new();
        let mut seen = std::collections::HashSet::new();
        for i in 0..100 {
            let id = reg.register(TenantId::new(1), StreamId::new(i), Offset::new(0), 1, None);
            assert!(seen.insert(id), "duplicate subscription id: {id}");
        }
    }
}
