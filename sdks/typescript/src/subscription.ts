/**
 * Real-time stream subscriptions (protocol v2).
 *
 * @example
 * ```ts
 * const sub = await client.subscribe(streamId, { fromOffset: 0n, initialCredits: 128 });
 * for await (const event of sub) {
 *   console.log(event.offset, event.data);
 * }
 * ```
 *
 * Subscriptions automatically replenish credits when the balance drops below
 * a low-water mark. Call `sub.unsubscribe()` to end iteration cleanly; the
 * for-await loop also terminates once the server emits a close event.
 */

import { Offset } from './types';
import { wrapNativeError } from './errors';
import type {
  NativeKimberliteClient,
  JsSubscriptionCloseReason,
} from './native';

export type SubscriptionCloseReason = JsSubscriptionCloseReason;

export interface SubscriptionEvent {
  offset: Offset;
  data: Buffer;
}

export interface SubscribeOptions {
  fromOffset?: Offset;
  initialCredits?: number;
  consumerGroup?: string;
  /**
   * When the credit balance drops below this threshold the subscription
   * auto-grants `refill` additional credits. Default: floor(initialCredits / 4).
   */
  lowWater?: number;
  /** Credits granted per auto-refill (default: initialCredits). */
  refill?: number;
}

interface SubscriptionState {
  native: NativeKimberliteClient;
  subscriptionId: bigint;
  credits: number;
  lowWater: number;
  refill: number;
  closed: boolean;
  closeReason: SubscriptionCloseReason | null;
}

export class Subscription implements AsyncIterable<SubscriptionEvent>, AsyncIterator<SubscriptionEvent> {
  private state: SubscriptionState;

  constructor(
    native: NativeKimberliteClient,
    subscriptionId: bigint,
    initialCredits: number,
    lowWater: number,
    refill: number,
  ) {
    this.state = {
      native,
      subscriptionId,
      credits: initialCredits,
      lowWater,
      refill: Math.max(refill, 1),
      closed: false,
      closeReason: null,
    };
  }

  get id(): bigint {
    return this.state.subscriptionId;
  }

  get credits(): number {
    return this.state.credits;
  }

  get closeReason(): SubscriptionCloseReason | null {
    return this.state.closeReason;
  }

  [Symbol.asyncIterator](): AsyncIterator<SubscriptionEvent> {
    return this;
  }

  async next(): Promise<IteratorResult<SubscriptionEvent>> {
    if (this.state.closed) {
      return { value: undefined, done: true };
    }
    await this.maybeAutoRefill();
    try {
      const ev = await this.state.native.nextSubscriptionEvent(this.state.subscriptionId);
      if (ev.closed) {
        this.state.closed = true;
        this.state.closeReason = ev.closeReason ?? null;
        return { value: undefined, done: true };
      }
      if (!ev.data) {
        // Defensive — native layer should never emit a non-closed event with null data.
        throw new Error('Subscription received event with null data');
      }
      // Credits bookkeeping: the native layer consumed one event, decrement our view.
      if (this.state.credits > 0) {
        this.state.credits -= 1;
      }
      return {
        value: { offset: ev.offset, data: ev.data },
        done: false,
      };
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async return(_value?: unknown): Promise<IteratorResult<SubscriptionEvent>> {
    await this.unsubscribe();
    return { value: undefined, done: true };
  }

  /** Grant additional credits synchronously. */
  async grantCredits(additional: number): Promise<number> {
    try {
      const newBalance = await this.state.native.grantCredits(
        this.state.subscriptionId,
        additional,
      );
      this.state.credits = newBalance;
      return newBalance;
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /** Cancel the subscription. Idempotent. */
  async unsubscribe(): Promise<void> {
    if (this.state.closed) return;
    this.state.closed = true;
    this.state.closeReason = 'ClientCancelled';
    try {
      await this.state.native.unsubscribe(this.state.subscriptionId);
    } catch (e) {
      // Best-effort — if the subscription was already closed by the server
      // we still want to surface `unsubscribe()` as idempotent.
      const err = e instanceof Error ? e : new Error(String(e));
      if (!/SubscriptionNotFound|SubscriptionClosed/.test(err.message)) {
        throw wrapNativeError(e);
      }
    }
  }

  private async maybeAutoRefill(): Promise<void> {
    if (this.state.credits <= this.state.lowWater && !this.state.closed) {
      await this.grantCredits(this.state.refill);
    }
  }
}
