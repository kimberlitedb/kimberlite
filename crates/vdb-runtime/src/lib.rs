//! vdb-runtime: Orchestrator for VerityDB
//!
//! The runtime ties together all components:
//! 1. Receive request (create_stream, append, etc.)
//! 2. Route to appropriate VSR group via directory
//! 3. Propose command to VSR
//! 4. On commit: apply to kernel, execute effects
//!
//! Generic over: App<R: GroupReplicator, X: Executor>

// TODO: Implement runtime orchestrator
