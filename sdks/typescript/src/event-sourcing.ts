/**
 * Event-sourcing primitives for the Kimberlite TypeScript SDK.
 *
 * AUDIT-2026-04 S4.14 — ports the `kimberlite-event-sourcing` Rust
 * crate's `apply` / `replay` / `runCommand` shape to TS. Before this
 * module, every downstream event-sourced app rebuilt the same
 * ~85 LoC of plumbing (notebar has it inside `repo-kit.ts`).
 *
 * The primitives are deliberately framework-agnostic: they describe
 * how a `(State, Event) -> State` fold composes with a
 * `(State, Command) -> Event[]` reducer on top of Kimberlite's
 * append-only stream — you plug in your own serialization format
 * (JSON, protobuf, msgpack) at the event ↔ `Buffer` boundary.
 *
 * @example
 * ```ts
 * type State = { count: number };
 * type Cmd = { type: 'Increment' } | { type: 'Reset' };
 * type Event = { type: 'Incremented' } | { type: 'Reset' };
 *
 * const handler: CommandHandler<State, Cmd, Event> = (state, cmd) => {
 *   if (cmd.type === 'Increment') return [{ type: 'Incremented' }];
 *   return [{ type: 'Reset' }];
 * };
 * const apply: Projector<State, Event> = (state, event) => {
 *   if (event.type === 'Incremented') return { count: state.count + 1 };
 *   return { count: 0 };
 * };
 *
 * const state0 = { count: 0 };
 * const codec = jsonCodec<Event>();
 * const state1 = await runCommand(client, streamId, state0, { type: 'Increment' }, handler, apply, codec);
 * ```
 */

import type { Client } from './client';
import type { StreamId, Offset } from './types';

/**
 * `(State, Event) -> State` fold — the canonical projector shape.
 * Pure; no IO.
 */
export type Projector<S, E> = (state: S, event: E, offset: Offset) => S;

/**
 * `(State, Command) -> Event[]` reducer — pure decision function.
 * Returns zero or more events to append when `cmd` is applied.
 * Throw to refuse the command.
 */
export type CommandHandler<S, C, E> = (state: S, cmd: C) => E[];

/**
 * Serialize / deserialize events to wire bytes. Use `jsonCodec` for
 * a batteries-included default.
 */
export interface EventCodec<E> {
  encode(event: E): Buffer;
  decode(bytes: Buffer): E;
}

/** JSON event codec — uses `JSON.stringify` / `JSON.parse`. */
export function jsonCodec<E>(): EventCodec<E> {
  return {
    encode(e: E): Buffer {
      return Buffer.from(JSON.stringify(e), 'utf8');
    },
    decode(b: Buffer): E {
      return JSON.parse(b.toString('utf8')) as E;
    },
  };
}

/**
 * Replay an entire stream from offset zero into `seed`. Returns the
 * fully-folded state.
 *
 * For long streams, prefer resume-from-offset patterns — snapshot
 * the state periodically, persist it alongside the last offset, and
 * on restart replay only the events after the snapshot offset.
 */
export async function replay<S, E>(
  client: Client,
  streamId: StreamId,
  seed: S,
  apply: Projector<S, E>,
  codec: EventCodec<E>,
  opts: { fromOffset?: Offset; maxBytes?: bigint } = {},
): Promise<{ state: S; offset: Offset }> {
  const from = opts.fromOffset ?? 0n;
  const maxBytes = opts.maxBytes ?? 16n * 1024n * 1024n;
  const events = await client.read(streamId, { fromOffset: from, maxBytes });
  let state = seed;
  let offset: Offset = from;
  for (const evt of events) {
    state = apply(state, codec.decode(evt.data), evt.offset);
    offset = evt.offset + 1n;
  }
  return { state, offset };
}

/**
 * Apply a command: replay the stream to compute current state,
 * run the command handler to get events, append them, fold them
 * forward, and return the new state.
 *
 * Callers that already hold the current state should use
 * {@link applyCommand} directly to skip the replay.
 */
export async function runCommand<S, C, E>(
  client: Client,
  streamId: StreamId,
  seed: S,
  cmd: C,
  handle: CommandHandler<S, C, E>,
  apply: Projector<S, E>,
  codec: EventCodec<E>,
): Promise<S> {
  const { state } = await replay(client, streamId, seed, apply, codec);
  return applyCommand(client, streamId, state, cmd, handle, apply, codec);
}

/**
 * Lower-level variant of {@link runCommand} that trusts the caller
 * to already hold the current state + expected offset. Useful when
 * the caller has a projection cache and doesn't want to re-read the
 * stream on every command.
 */
export async function applyCommand<S, C, E>(
  client: Client,
  streamId: StreamId,
  state: S,
  cmd: C,
  handle: CommandHandler<S, C, E>,
  apply: Projector<S, E>,
  codec: EventCodec<E>,
): Promise<S> {
  const events = handle(state, cmd);
  if (events.length === 0) {
    return state;
  }
  const payloads = events.map((e) => codec.encode(e));
  const firstOffset = await client.append(streamId, payloads);
  let next = state;
  for (let i = 0; i < events.length; i++) {
    next = apply(next, events[i]!, firstOffset + BigInt(i));
  }
  return next;
}
