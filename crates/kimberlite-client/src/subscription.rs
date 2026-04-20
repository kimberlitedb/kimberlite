//! Real-time stream subscriptions (protocol v2).
//!
//! A [`Subscription`] borrows a [`Client`] and yields events as the server
//! pushes them. Use [`Client::subscribe`] to create one:
//!
//! ```ignore
//! use kimberlite_client::{Client, Subscription};
//! use kimberlite_types::{Offset, StreamId};
//!
//! let ack = client.subscribe(StreamId::new(1), Offset::new(0), 128, None)?;
//! let mut sub = Subscription::new(&mut client, ack.subscription_id, 128);
//!
//! while let Some(event) = sub.next_event()? {
//!     println!("offset {}: {:?}", event.offset, event.data);
//! }
//! ```
//!
//! The subscription automatically replenishes credits when the balance drops
//! below the low-water mark. Callers can override with [`Subscription::grant_credits`].

use kimberlite_types::Offset;
use kimberlite_wire::{PushPayload, SubscriptionCloseReason};

use crate::client::Client;
use crate::error::{ClientError, ClientResult};

/// A single event delivered on a subscription.
#[derive(Debug, Clone)]
pub struct SubscriptionEvent {
    /// Offset of this event in the source stream.
    pub offset: Offset,
    /// Event payload bytes.
    pub data: Vec<u8>,
}

/// Borrow-based subscription iterator.
///
/// The subscription lives as long as the mutable borrow of the underlying
/// `Client`. Because the Rust client is synchronous and single-threaded,
/// a client can have at most one active `Subscription` at a time — use
/// [`Pool`](crate::Pool) for concurrent subscriptions.
#[allow(clippy::struct_field_names)] // `subscription_id` matches the wire field
pub struct Subscription<'c> {
    client: &'c mut Client,
    subscription_id: u64,
    /// Current local view of the server's credit balance.
    credits: u32,
    /// Auto-grant threshold: when `credits` drops below this value the
    /// subscription automatically requests more.
    low_water: u32,
    /// Amount granted per auto-replenish.
    refill: u32,
    /// Events from the most recent push, not yet yielded.
    pending: std::collections::VecDeque<SubscriptionEvent>,
    /// True once a `SubscriptionClosed` frame has arrived or `close()` has
    /// been called. Further `next_event` calls return `Ok(None)`.
    closed: Option<SubscriptionCloseReason>,
}

impl<'c> Subscription<'c> {
    /// Create a subscription handle from a pre-issued subscription ID.
    ///
    /// Prefer [`Client::subscribe_iter`] which wraps the two-step handshake.
    pub fn new(client: &'c mut Client, subscription_id: u64, initial_credits: u32) -> Self {
        let refill = initial_credits.max(1);
        let low_water = refill / 4;
        Self {
            client,
            subscription_id,
            credits: initial_credits,
            low_water,
            refill,
            pending: std::collections::VecDeque::new(),
            closed: None,
        }
    }

    /// Configures automatic credit replenishment.
    ///
    /// When the local credit balance drops below `low_water`, the
    /// subscription automatically sends a `SubscribeCredit` request for
    /// `refill` additional credits.
    pub fn with_auto_refill(mut self, low_water: u32, refill: u32) -> Self {
        self.low_water = low_water;
        self.refill = refill.max(1);
        self
    }

    /// Returns the server-assigned subscription ID.
    pub fn id(&self) -> u64 {
        self.subscription_id
    }

    /// Current local credit balance.
    pub fn credits(&self) -> u32 {
        self.credits
    }

    /// `Some(reason)` if the subscription has been closed.
    pub fn close_reason(&self) -> Option<SubscriptionCloseReason> {
        self.closed
    }

    /// Grant `additional` credits to this subscription synchronously.
    ///
    /// The server returns the new balance, which replaces the local view.
    pub fn grant_credits(&mut self, additional: u32) -> ClientResult<u32> {
        let new_balance = self
            .client
            .grant_credits(self.subscription_id, additional)?;
        self.credits = new_balance;
        Ok(new_balance)
    }

    /// Returns the next event, blocking until one arrives or the
    /// subscription closes. Returns `Ok(None)` once the subscription is
    /// closed.
    pub fn next_event(&mut self) -> ClientResult<Option<SubscriptionEvent>> {
        // Drain pending batch first.
        if let Some(ev) = self.pending.pop_front() {
            return Ok(Some(ev));
        }
        if self.closed.is_some() {
            return Ok(None);
        }

        self.maybe_auto_refill()?;

        loop {
            let push = match self.client.next_push()? {
                Some(p) => p,
                None => {
                    // EOF / no more data.
                    return Ok(None);
                }
            };
            match push.payload {
                PushPayload::SubscriptionEvents {
                    subscription_id,
                    start_offset,
                    events,
                    credits_remaining,
                } if subscription_id == self.subscription_id => {
                    self.credits = credits_remaining;
                    for (i, data) in events.into_iter().enumerate() {
                        self.pending.push_back(SubscriptionEvent {
                            offset: Offset::new(start_offset.as_u64() + i as u64),
                            data,
                        });
                    }
                    if let Some(ev) = self.pending.pop_front() {
                        return Ok(Some(ev));
                    }
                }
                PushPayload::SubscriptionClosed {
                    subscription_id,
                    reason,
                } if subscription_id == self.subscription_id => {
                    self.closed = Some(reason);
                    return Ok(None);
                }
                // Push for another subscription — drop on the floor. With a
                // single-subscription model this is unreachable; kept here
                // defensively.
                _ => {
                    tracing::trace!(
                        "subscription {}: ignoring push for another subscription",
                        self.subscription_id
                    );
                }
            }
        }
    }

    /// Cancels the subscription. Consumes self so callers can't accidentally
    /// re-use a closed handle.
    pub fn unsubscribe(self) -> ClientResult<()> {
        self.client.unsubscribe(self.subscription_id)
    }

    fn maybe_auto_refill(&mut self) -> ClientResult<()> {
        if self.credits <= self.low_water && self.closed.is_none() {
            self.grant_credits(self.refill)?;
        }
        Ok(())
    }
}

impl Client {
    /// Subscribe-and-wrap helper — creates a [`Subscription`] from a stream
    /// plus auto-refill settings in a single call.
    pub fn subscribe_iter(
        &mut self,
        stream_id: kimberlite_types::StreamId,
        from_offset: Offset,
        initial_credits: u32,
        consumer_group: Option<String>,
    ) -> ClientResult<Subscription<'_>> {
        let ack = self.subscribe(stream_id, from_offset, initial_credits, consumer_group)?;
        if ack.credits == 0 {
            return Err(ClientError::server(
                kimberlite_wire::ErrorCode::SubscriptionBackpressure,
                "subscription created with zero initial credits",
            ));
        }
        Ok(Subscription::new(self, ack.subscription_id, ack.credits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_event_has_expected_shape() {
        let ev = SubscriptionEvent {
            offset: Offset::new(42),
            data: b"hello".to_vec(),
        };
        assert_eq!(ev.offset.as_u64(), 42);
        assert_eq!(ev.data, b"hello");
    }
}
