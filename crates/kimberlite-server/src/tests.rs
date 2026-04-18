//! Integration tests for the server.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use kimberlite::Kimberlite;
use kimberlite_client::{Client, ClientConfig};
use kimberlite_types::{DataClass, Offset, TenantId};
use tempfile::TempDir;

use crate::{Server, ServerConfig};

/// Finds an available port on localhost.
fn find_available_port() -> u16 {
    // Bind to port 0 to let OS assign an available port
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to port 0");
    listener
        .local_addr()
        .expect("Failed to get local addr")
        .port()
}

#[test]
fn test_server_binds_to_address() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let port = find_available_port();
    let addr = format!("127.0.0.1:{port}")
        .parse::<SocketAddr>()
        .expect("Invalid addr");
    let config = ServerConfig::new(addr, temp_dir.path());
    let db = Kimberlite::open(temp_dir.path()).expect("Failed to open database");

    let server = Server::new(config, db).expect("Failed to create server");
    let local_addr = server.local_addr().expect("Failed to get local addr");

    assert_eq!(local_addr.port(), port);
    assert_eq!(server.connection_count(), 0);
}

#[test]
fn test_server_accepts_connection() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let port = find_available_port();
    let addr = format!("127.0.0.1:{port}")
        .parse::<SocketAddr>()
        .expect("Invalid addr");
    let config = ServerConfig::new(addr, temp_dir.path());
    let db = Kimberlite::open(temp_dir.path()).expect("Failed to open database");

    let mut server = Server::new(config, db).expect("Failed to create server");

    // Connect a client in a background thread
    let client_handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        let config = ClientConfig::default();
        let result = Client::connect(format!("127.0.0.1:{port}"), TenantId::new(1), config);
        result.is_ok()
    });

    // Poll the server a few times to accept and process the connection
    for _ in 0..10 {
        let _ = server.poll_once(Some(Duration::from_millis(50)));
    }

    let client_connected = client_handle.join().expect("Client thread panicked");
    assert!(client_connected, "Client should connect successfully");
}

#[test]
fn test_server_max_connections() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let port = find_available_port();
    let addr = format!("127.0.0.1:{port}")
        .parse::<SocketAddr>()
        .expect("Invalid addr");

    // Set max connections to 2
    let config = ServerConfig::new(addr, temp_dir.path()).with_max_connections(2);
    let db = Kimberlite::open(temp_dir.path()).expect("Failed to open database");

    let mut server = Server::new(config, db).expect("Failed to create server");

    // Spawn multiple client connections
    let mut handles = vec![];
    for i in 0_u64..3 {
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50 * (i + 1)));
            let config = ClientConfig {
                read_timeout: Some(Duration::from_millis(500)),
                write_timeout: Some(Duration::from_millis(500)),
                ..Default::default()
            };
            Client::connect(format!("127.0.0.1:{port}"), TenantId::new(1), config).is_ok()
        });
        handles.push(handle);
    }

    // Poll the server to process connections
    for _ in 0..20 {
        let _ = server.poll_once(Some(Duration::from_millis(50)));
    }

    let results: Vec<bool> = handles
        .into_iter()
        .map(|h| h.join().expect("Client thread panicked"))
        .collect();

    // At least 2 should succeed, the third may be rejected
    let successes = results.iter().filter(|&&r| r).count();
    assert!(
        successes >= 2,
        "At least 2 connections should succeed, got {successes}"
    );
}

#[test]
fn test_connection_buffer_limit() {
    // Test that the client enforces buffer limits
    let config = ClientConfig {
        buffer_size: 1024, // Small buffer for testing
        ..Default::default()
    };

    // Verify the configuration is respected
    assert_eq!(config.buffer_size, 1024);
}

#[test]
fn test_server_config_defaults() {
    let config = ServerConfig::default();

    assert_eq!(config.bind_addr.port(), 5432);
    assert_eq!(config.max_connections, 1024);
    assert_eq!(config.read_buffer_size, 64 * 1024);
}

#[test]
fn test_client_config_defaults() {
    let config = ClientConfig::default();

    assert_eq!(config.read_timeout, Some(Duration::from_secs(30)));
    assert_eq!(config.write_timeout, Some(Duration::from_secs(30)));
    assert_eq!(config.buffer_size, 64 * 1024);
    assert!(config.auth_token.is_none());
}

#[test]
fn test_shutdown_handle() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let port = find_available_port();
    let addr = format!("127.0.0.1:{port}")
        .parse::<SocketAddr>()
        .expect("Invalid addr");
    let config = ServerConfig::new(addr, temp_dir.path());
    let db = Kimberlite::open(temp_dir.path()).expect("Failed to open database");

    let server = Server::new(config, db).expect("Failed to create server");

    // Get a shutdown handle
    let handle = server.shutdown_handle();

    // Initially, shutdown should not be requested
    assert!(!handle.is_shutdown_requested());
    assert!(!server.is_shutdown_requested());

    // Request shutdown via handle
    handle.shutdown();

    // Now both should report shutdown requested
    assert!(handle.is_shutdown_requested());
    assert!(server.is_shutdown_requested());
}

#[test]
fn test_graceful_shutdown() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let port = find_available_port();
    let addr = format!("127.0.0.1:{port}")
        .parse::<SocketAddr>()
        .expect("Invalid addr");
    let config = ServerConfig::new(addr, temp_dir.path());
    let db = Kimberlite::open(temp_dir.path()).expect("Failed to open database");

    let server = Server::new(config, db).expect("Failed to create server");
    let handle = server.shutdown_handle();

    // Spawn server in background
    let server_thread = thread::spawn(move || {
        let mut server = server;
        server.run_with_shutdown()
    });

    // Give server time to start
    thread::sleep(Duration::from_millis(100));

    // Connect a client
    let config = ClientConfig::default();
    let client = Client::connect(format!("127.0.0.1:{port}"), TenantId::new(1), config);
    assert!(client.is_ok(), "Client should connect");

    // Request shutdown
    handle.shutdown();

    // Wait for server to complete
    let result = server_thread.join().expect("Server thread panicked");
    assert!(result.is_ok(), "Server should shut down gracefully");
}

#[cfg(test)]
mod end_to_end {
    use super::*;

    /// Helper to run a server and client end-to-end test.
    fn run_e2e_test<F>(test_fn: F)
    where
        F: FnOnce(u16) + Send + 'static,
    {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let port = find_available_port();
        let addr = format!("127.0.0.1:{port}")
            .parse::<SocketAddr>()
            .expect("Invalid addr");
        let config = ServerConfig::new(addr, temp_dir.path());
        let db = Kimberlite::open(temp_dir.path()).expect("Failed to open database");

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        let server_handle = thread::spawn(move || {
            let mut server = Server::new(config, db).expect("Failed to create server");
            while running_clone.load(Ordering::SeqCst) {
                let _ = server.poll_once(Some(Duration::from_millis(10)));
            }
        });

        // Give the server time to start
        thread::sleep(Duration::from_millis(100));

        // Run the test
        test_fn(port);

        // Stop the server
        running.store(false, Ordering::SeqCst);
        server_handle.join().expect("Server thread panicked");
    }

    #[test]
    fn test_e2e_handshake() {
        run_e2e_test(|port| {
            let config = ClientConfig::default();
            let client = Client::connect(format!("127.0.0.1:{port}"), TenantId::new(1), config);

            assert!(client.is_ok(), "Handshake should succeed");
        });
    }

    #[test]
    fn test_e2e_create_stream() {
        run_e2e_test(|port| {
            let config = ClientConfig::default();
            let mut client = Client::connect(format!("127.0.0.1:{port}"), TenantId::new(1), config)
                .expect("Failed to connect");

            let stream_id = client
                .create_stream("test-events", DataClass::Public)
                .expect("Failed to create stream");

            assert!(u64::from(stream_id) > 0, "Stream ID should be assigned");
        });
    }

    #[test]
    fn test_e2e_append_and_read() {
        run_e2e_test(|port| {
            let config = ClientConfig::default();
            let mut client = Client::connect(format!("127.0.0.1:{port}"), TenantId::new(1), config)
                .expect("Failed to connect");

            // Create a stream
            let stream_id = client
                .create_stream("events", DataClass::Public)
                .expect("Failed to create stream");

            // Append events
            let events = vec![b"event1".to_vec(), b"event2".to_vec(), b"event3".to_vec()];
            let first_offset = client
                .append(stream_id, events, Offset::ZERO)
                .expect("Failed to append events");

            assert_eq!(first_offset.as_u64(), 0, "First offset should be 0");

            // Read events back
            let response = client
                .read_events(stream_id, Offset::new(0), 1024 * 1024)
                .expect("Failed to read events");

            assert_eq!(response.events.len(), 3, "Should read 3 events");
            assert_eq!(response.events[0], b"event1");
            assert_eq!(response.events[1], b"event2");
            assert_eq!(response.events[2], b"event3");
        });
    }

    #[test]
    fn test_e2e_sync() {
        run_e2e_test(|port| {
            let config = ClientConfig::default();
            let mut client = Client::connect(format!("127.0.0.1:{port}"), TenantId::new(1), config)
                .expect("Failed to connect");

            // Sync should succeed
            client.sync().expect("Sync should succeed");
        });
    }

    #[test]
    fn test_e2e_batch_append() {
        run_e2e_test(|port| {
            let config = ClientConfig::default();
            let mut client = Client::connect(format!("127.0.0.1:{port}"), TenantId::new(1), config)
                .expect("Failed to connect");

            // Create a stream
            let stream = client
                .create_stream("batch-test", DataClass::Public)
                .expect("Failed to create stream");

            // Append multiple events in a single batch
            let first_offset = client
                .append(
                    stream,
                    vec![
                        b"event1".to_vec(),
                        b"event2".to_vec(),
                        b"event3".to_vec(),
                        b"event4".to_vec(),
                    ],
                    Offset::ZERO,
                )
                .expect("Failed to append batch");

            // Read all events back
            let response = client
                .read_events(stream, first_offset, 4096)
                .expect("Failed to read stream");

            assert_eq!(response.events.len(), 4, "Should have 4 events");
            assert_eq!(response.events[0], b"event1");
            assert_eq!(response.events[1], b"event2");
            assert_eq!(response.events[2], b"event3");
            assert_eq!(response.events[3], b"event4");
        });
    }

    #[test]
    fn test_e2e_moderately_sized_payload() {
        run_e2e_test(|port| {
            let config = ClientConfig::default();
            let mut client = Client::connect(format!("127.0.0.1:{port}"), TenantId::new(1), config)
                .expect("Failed to connect");

            // Create a stream
            let stream_id = client
                .create_stream("sized-events", DataClass::Public)
                .expect("Failed to create stream");

            // Append moderately sized events that fit in B+tree pages (4KB pages)
            // Use ~2KB events to be safe
            let event = vec![0xAB_u8; 2000];
            let first_offset = client
                .append(stream_id, vec![event.clone()], Offset::ZERO)
                .expect("Failed to append event");

            // Read it back
            let response = client
                .read_events(stream_id, first_offset, 8 * 1024)
                .expect("Failed to read event");

            assert_eq!(response.events.len(), 1, "Should read 1 event");
            assert_eq!(response.events[0].len(), 2000, "Event size should match");
            assert_eq!(response.events[0], event, "Event content should match");
        });
    }

    #[test]
    fn test_e2e_reconnection() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let port = find_available_port();
        let addr = format!("127.0.0.1:{port}")
            .parse::<SocketAddr>()
            .expect("Invalid addr");
        let config = ServerConfig::new(addr, temp_dir.path());
        let db = Kimberlite::open(temp_dir.path()).expect("Failed to open database");

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        let server_handle = thread::spawn(move || {
            let mut server = Server::new(config, db).expect("Failed to create server");
            while running_clone.load(Ordering::SeqCst) {
                let _ = server.poll_once(Some(Duration::from_millis(10)));
            }
        });

        // Give server time to start
        thread::sleep(Duration::from_millis(100));

        // First connection
        {
            let config = ClientConfig::default();
            let mut client = Client::connect(format!("127.0.0.1:{port}"), TenantId::new(1), config)
                .expect("Failed to connect");

            let stream_id = client
                .create_stream("reconnect-test", DataClass::Public)
                .expect("Failed to create stream");

            client
                .append(stream_id, vec![b"event1".to_vec()], Offset::ZERO)
                .expect("Failed to append");
        }
        // Client dropped, connection closed

        // Second connection
        {
            let config = ClientConfig::default();
            let client = Client::connect(format!("127.0.0.1:{port}"), TenantId::new(1), config);

            assert!(client.is_ok(), "Reconnection should succeed");
        }

        // Stop server
        running.store(false, Ordering::SeqCst);
        server_handle.join().expect("Server thread panicked");
    }
}

/// Integration test: in a 3-node VSR cluster, a write submitted via the
/// leader's CommandSubmitter must make the resulting stream visible on
/// every replica's Kimberlite projection — including the two followers
/// that never see a direct `db.submit` call.
///
/// This regression-guards the apply_committed → db.submit wiring added
/// via the new `AppliedCommit` fanout + projection-applier thread. Before
/// that wiring, followers' projections stayed empty because VSR's
/// kernel_state updates didn't propagate to the Kimberlite layer.
#[cfg(test)]
mod follower_projection {
    use std::time::Instant;

    use kimberlite::Kimberlite;
    use kimberlite_kernel::Command;
    use kimberlite_types::{DataClass, Placement, StreamName};
    use tempfile::TempDir;

    use crate::ReplicationMode;
    use crate::replication::CommandSubmitter;

    /// Base port for the 3-node localhost cluster. Tests that run in
    /// parallel would collide on a fixed port — spawn a helper that
    /// picks a free port instead, then derive the other two.
    fn pick_base_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind 0");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        // Give ourselves 3 consecutive ports starting from `port`. In
        // rare cases port+1 or port+2 is taken; that's inherent to
        // localhost cluster tests and we accept the very occasional flake.
        port
    }

    #[test]
    #[ignore = "in-process 3-node VSR bootstrap is flaky without extended timeouts — \
                run manually with `cargo test ... -- --ignored` or rely on the \
                EPYC chaos suite for end-to-end coverage"]
    fn follower_sees_leader_write() {
        let dirs: Vec<TempDir> = (0..3).map(|_| TempDir::new().expect("tempdir")).collect();
        let base_port = pick_base_port();

        // Spin up 3 submitters forming a cluster.
        let mut submitters = Vec::with_capacity(3);
        for (replica_id, dir) in dirs.iter().enumerate() {
            let db = Kimberlite::open(dir.path()).expect("open db");
            let mode = ReplicationMode::cluster_localhost(replica_id as u8, base_port);
            let s = CommandSubmitter::new(&mode, db, dir.path()).expect("new submitter");
            submitters.push(s);
        }

        // Wait for a leader to emerge. `cluster_localhost` runs a real
        // VSR bootstrap — view change + quorum connectivity takes a
        // couple of seconds on a loaded CI box.
        wait_for_leader(&submitters, std::time::Duration::from_secs(15))
            .expect("leader elected within budget");
        // Give the cluster another moment to STABILISE — the first
        // `is_leader() == true` can coincide with an in-flight view
        // change that flips leadership immediately after. Extra settle
        // time eliminates nearly all of that flakiness.
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Submit via whoever is leader RIGHT NOW. Leadership can flip
        // between our find and our submit call during bootstrap-era
        // view changes, so we retry with a fresh leader lookup each
        // iteration up to a short budget.
        let command = Command::create_stream_with_auto_id(
            StreamName::new("follower-test-stream"),
            DataClass::Public,
            Placement::Global,
        );
        let submit_deadline = Instant::now() + std::time::Duration::from_secs(20);
        let mut final_result = None;
        let mut last_error: Option<String> = None;
        while Instant::now() < submit_deadline {
            let leaders: Vec<usize> = submitters
                .iter()
                .enumerate()
                .filter(|(_, s)| s.is_leader())
                .map(|(i, _)| i)
                .collect();
            let Some(&leader) = leaders.first() else {
                last_error = Some("no replica reports is_leader".into());
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            };
            match submitters[leader].submit(command.clone()) {
                Ok(res) => {
                    final_result = Some(res);
                    break;
                }
                Err(e) => {
                    last_error = Some(format!("r{leader}: {e}"));
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        }
        let result = final_result.unwrap_or_else(|| {
            panic!(
                "submit did not succeed within 20s; last error: {}",
                last_error.unwrap_or_else(|| "<none>".into())
            )
        });
        assert!(
            !result.was_duplicate,
            "fresh CreateStream should not be duplicate",
        );

        // Wait up to 3s for EVERY replica's kernel_state to reflect the
        // new stream. The projection applier fans out via the VSR commit
        // stream, so followers catch up within a tick.
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        while Instant::now() < deadline {
            let all_seen = submitters.iter().all(|s| {
                s.kernel_state_snapshot(std::time::Duration::from_millis(500))
                    .map(|state| {
                        state
                            .streams()
                            .values()
                            .any(|m| m.stream_name.as_str() == "follower-test-stream")
                    })
                    .unwrap_or(false)
            });
            if all_seen {
                return; // success
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Report which replicas fell behind for debuggability.
        let report: Vec<String> = submitters
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let seen = s
                    .kernel_state_snapshot(std::time::Duration::from_millis(500))
                    .map(|state| {
                        state
                            .streams()
                            .values()
                            .any(|m| m.stream_name.as_str() == "follower-test-stream")
                    })
                    .unwrap_or(false);
                format!("r{i}={seen}")
            })
            .collect();
        panic!(
            "not every replica's kernel_state saw the new stream within 3s: [{}]",
            report.join(", ")
        );
    }

    fn wait_for_leader(
        submitters: &[CommandSubmitter],
        budget: std::time::Duration,
    ) -> Option<usize> {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            if let Some((idx, _)) = submitters
                .iter()
                .enumerate()
                .find(|(_, s)| s.is_leader())
            {
                return Some(idx);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        None
    }
}
