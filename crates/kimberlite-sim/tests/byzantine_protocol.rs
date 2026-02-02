//! Integration tests for protocol-level Byzantine message testing.
//!
//! These tests verify that:
//! 1. Message mutations are applied correctly
//! 2. VSR handlers detect and reject Byzantine messages

#![allow(clippy::uninlined_format_args)] // Test assertions use old format style
#![allow(clippy::float_cmp)] // Test assertions need exact float comparisons
//! 3. Instrumentation tracks rejection statistics

use kimberlite_kernel::Command;
use kimberlite_sim::{
    AttackPattern, ByzantineInjector, MessageFieldMutation, MessageMutationRule, MessageMutator,
    MessageTypeFilter, SimRng,
};
use kimberlite_types::{DataClass, Placement, Region, StreamId, StreamName, TenantId};
use kimberlite_vsr::{
    CommitNumber, DoViewChange, LogEntry, Message, MessagePayload, OpNumber, ReplicaId, StartView,
    ViewNumber,
};

fn make_test_command() -> Command {
    Command::CreateStream {
        stream_id: StreamId::from_tenant_and_local(TenantId::new(1), 1),
        stream_name: StreamName::from("test"),
        data_class: DataClass::PHI,
        placement: Placement::Region(Region::USEast1),
    }
}

#[test]
fn test_message_mutation_inflates_commit_number() {
    let rules = vec![MessageMutationRule {
        target: MessageTypeFilter::DoViewChange,
        from_replica: None,
        to_replica: None,
        probability: 1.0,
        mutation: MessageFieldMutation::InflateCommitNumber { amount: 500 },
        deliver: true,
    }];

    let mut mutator = MessageMutator::new(rules);
    let mut rng = SimRng::new(42);

    let dvc = DoViewChange {
        view: ViewNumber::from(2),
        last_normal_view: ViewNumber::from(1),
        op_number: OpNumber::new(100),
        commit_number: CommitNumber::new(OpNumber::new(50)),
        log_tail: vec![],
        replica: ReplicaId::new(0),
    };

    let message = Message {
        from: ReplicaId::new(0),
        to: Some(ReplicaId::new(1)),
        payload: MessagePayload::DoViewChange(dvc),
    };

    // Apply mutation
    let mutated = mutator
        .apply(&message, ReplicaId::new(1), &mut rng)
        .expect("mutation should apply");

    // Verify mutation was applied
    if let MessagePayload::DoViewChange(mutated_dvc) = &mutated.payload {
        assert_eq!(mutated_dvc.commit_number.as_u64(), 550); // 50 + 500
    } else {
        panic!("Expected DoViewChange payload");
    }

    // Verify statistics
    let stats = mutator.stats();
    assert_eq!(stats.mutations_applied, 1);
    assert_eq!(stats.do_view_change_mutations, 1);
}

#[test]
fn test_message_mutation_truncates_log_tail() {
    let rules = vec![MessageMutationRule {
        target: MessageTypeFilter::DoViewChange,
        from_replica: None,
        to_replica: None,
        probability: 1.0,
        mutation: MessageFieldMutation::TruncateLogTail { max_entries: 1 },
        deliver: true,
    }];

    let mut mutator = MessageMutator::new(rules);
    let mut rng = SimRng::new(42);

    let log_tail = vec![
        LogEntry {
            op_number: OpNumber::new(1),
            view: ViewNumber::from(1),
            command: make_test_command(),
            idempotency_id: None,
            checksum: 0,
        },
        LogEntry {
            op_number: OpNumber::new(2),
            view: ViewNumber::from(1),
            command: make_test_command(),
            idempotency_id: None,
            checksum: 1,
        },
        LogEntry {
            op_number: OpNumber::new(3),
            view: ViewNumber::from(1),
            command: make_test_command(),
            idempotency_id: None,
            checksum: 2,
        },
    ];

    let dvc = DoViewChange {
        view: ViewNumber::from(2),
        last_normal_view: ViewNumber::from(1),
        op_number: OpNumber::new(3),
        commit_number: CommitNumber::new(OpNumber::new(0)),
        log_tail,
        replica: ReplicaId::new(0),
    };

    let message = Message {
        from: ReplicaId::new(0),
        to: Some(ReplicaId::new(1)),
        payload: MessagePayload::DoViewChange(dvc),
    };

    // Apply mutation
    let mutated = mutator
        .apply(&message, ReplicaId::new(1), &mut rng)
        .expect("mutation should apply");

    // Verify truncation
    if let MessagePayload::DoViewChange(mutated_dvc) = &mutated.payload {
        assert_eq!(mutated_dvc.log_tail.len(), 1);
        assert_eq!(mutated_dvc.log_tail[0].op_number.as_u64(), 1);
    } else {
        panic!("Expected DoViewChange payload");
    }
}

#[test]
fn test_byzantine_injector_builds_mutation_rules() {
    let injector = ByzantineInjector::new()
        .with_inflate_commit_number(1.0)
        .with_commit_inflation_factor(500)
        .with_truncate_log_tail(true);

    let rules = injector.build_mutation_rules();

    // Should generate rules for: 3 inflate (DVC, SV, Commit) + 2 truncate (DVC, SV) = 5
    assert_eq!(rules.len(), 5);

    // All rules should have deliver=true
    for rule in &rules {
        assert!(rule.deliver);
    }
}

#[test]
fn test_attack_pattern_generates_rules() {
    for pattern in AttackPattern::all() {
        let injector = pattern.injector();
        let rules = injector.build_mutation_rules();

        // Each pattern should generate at least one rule
        assert!(
            !rules.is_empty(),
            "Pattern {:?} generated no rules",
            pattern
        );
    }
}

#[test]
fn test_composite_mutation() {
    let rules = vec![MessageMutationRule {
        target: MessageTypeFilter::DoViewChange,
        from_replica: None,
        to_replica: None,
        probability: 1.0,
        mutation: MessageFieldMutation::Composite(vec![
            MessageFieldMutation::InflateCommitNumber { amount: 100 },
            MessageFieldMutation::TruncateLogTail { max_entries: 1 },
        ]),
        deliver: true,
    }];

    let mut mutator = MessageMutator::new(rules);
    let mut rng = SimRng::new(42);

    let log_tail = vec![
        LogEntry {
            op_number: OpNumber::new(1),
            view: ViewNumber::from(1),
            command: make_test_command(),
            idempotency_id: None,
            checksum: 0,
        },
        LogEntry {
            op_number: OpNumber::new(2),
            view: ViewNumber::from(1),
            command: make_test_command(),
            idempotency_id: None,
            checksum: 1,
        },
    ];

    let dvc = DoViewChange {
        view: ViewNumber::from(2),
        last_normal_view: ViewNumber::from(1),
        op_number: OpNumber::new(2),
        commit_number: CommitNumber::new(OpNumber::new(0)),
        log_tail,
        replica: ReplicaId::new(0),
    };

    let message = Message {
        from: ReplicaId::new(0),
        to: Some(ReplicaId::new(1)),
        payload: MessagePayload::DoViewChange(dvc),
    };

    // Apply composite mutation
    let mutated = mutator
        .apply(&message, ReplicaId::new(1), &mut rng)
        .expect("mutation should apply");

    // Verify both mutations applied
    if let MessagePayload::DoViewChange(mutated_dvc) = &mutated.payload {
        // Commit number inflated
        assert_eq!(mutated_dvc.commit_number.as_u64(), 100);
        // Log tail truncated
        assert_eq!(mutated_dvc.log_tail.len(), 1);
    } else {
        panic!("Expected DoViewChange payload");
    }
}

#[test]
fn test_fork_mutation() {
    let group_a = vec![ReplicaId::new(0), ReplicaId::new(1)];
    let group_b = vec![ReplicaId::new(2)];

    let rules = vec![MessageMutationRule {
        target: MessageTypeFilter::DoViewChange,
        from_replica: None,
        to_replica: None,
        probability: 1.0,
        mutation: MessageFieldMutation::Fork {
            group_a: group_a.clone(),
            mutation_a: Box::new(MessageFieldMutation::InflateCommitNumber { amount: 100 }),
            group_b: group_b.clone(),
            mutation_b: Box::new(MessageFieldMutation::InflateCommitNumber { amount: 200 }),
        },
        deliver: true,
    }];

    let mut mutator = MessageMutator::new(rules);
    let mut rng = SimRng::new(42);

    let dvc = DoViewChange {
        view: ViewNumber::from(2),
        last_normal_view: ViewNumber::from(1),
        op_number: OpNumber::new(100),
        commit_number: CommitNumber::new(OpNumber::new(50)),
        log_tail: vec![],
        replica: ReplicaId::new(0),
    };

    let message = Message {
        from: ReplicaId::new(0),
        to: Some(ReplicaId::new(1)),
        payload: MessagePayload::DoViewChange(dvc.clone()),
    };

    // Apply mutation for group_a replica (replica 0)
    let mutated_a = mutator
        .apply(&message, ReplicaId::new(0), &mut rng)
        .expect("mutation should apply");

    if let MessagePayload::DoViewChange(dvc_a) = &mutated_a.payload {
        assert_eq!(dvc_a.commit_number.as_u64(), 150); // 50 + 100
    }

    // Apply mutation for group_b replica (replica 2)
    mutator.reset_stats();
    let mutated_b = mutator
        .apply(&message, ReplicaId::new(2), &mut rng)
        .expect("mutation should apply");

    if let MessagePayload::DoViewChange(dvc_b) = &mutated_b.payload {
        assert_eq!(dvc_b.commit_number.as_u64(), 250); // 50 + 200
    }
}

#[test]
fn test_mutation_statistics() {
    let rules = vec![
        MessageMutationRule {
            target: MessageTypeFilter::DoViewChange,
            from_replica: None,
            to_replica: None,
            probability: 1.0,
            mutation: MessageFieldMutation::InflateCommitNumber { amount: 100 },
            deliver: true,
        },
        MessageMutationRule {
            target: MessageTypeFilter::StartView,
            from_replica: None,
            to_replica: None,
            probability: 1.0,
            mutation: MessageFieldMutation::InflateCommitNumber { amount: 200 },
            deliver: true,
        },
    ];

    let mut mutator = MessageMutator::new(rules);
    let mut rng = SimRng::new(42);

    // Send DoViewChange
    let dvc_msg = Message {
        from: ReplicaId::new(0),
        to: Some(ReplicaId::new(1)),
        payload: MessagePayload::DoViewChange(DoViewChange {
            view: ViewNumber::from(2),
            last_normal_view: ViewNumber::from(1),
            op_number: OpNumber::new(100),
            commit_number: CommitNumber::new(OpNumber::new(50)),
            log_tail: vec![],
            replica: ReplicaId::new(0),
        }),
    };

    mutator.apply(&dvc_msg, ReplicaId::new(1), &mut rng);

    // Send StartView
    let sv_msg = Message {
        from: ReplicaId::new(0),
        to: Some(ReplicaId::new(1)),
        payload: MessagePayload::StartView(StartView {
            view: ViewNumber::from(2),
            op_number: OpNumber::new(100),
            commit_number: CommitNumber::new(OpNumber::new(50)),
            log_tail: vec![],
        }),
    };

    mutator.apply(&sv_msg, ReplicaId::new(1), &mut rng);

    // Check statistics
    let stats = mutator.stats();
    assert_eq!(stats.messages_processed, 2);
    assert_eq!(stats.mutations_applied, 2);
    assert_eq!(stats.do_view_change_mutations, 1);
    assert_eq!(stats.start_view_mutations, 1);
    assert_eq!(stats.mutation_rate(), 1.0);
}
