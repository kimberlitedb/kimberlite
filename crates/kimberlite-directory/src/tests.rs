//! Unit tests for kmb-directory

use std::sync::Arc;
use std::thread;

use kimberlite_types::{GroupId, Placement, Region};

use crate::{Directory, DirectoryError};

// ============================================================================
// Directory Tests
// ============================================================================

#[test]
fn global_placement_returns_global_group() {
    let directory = Directory::new(GroupId::new(0));
    let group = directory.group_for_placement(&Placement::Global).unwrap();
    assert_eq!(group, GroupId::new(0));
}

#[test]
fn regional_placement_returns_regional_group() {
    let directory = Directory::new(GroupId::new(0))
        .with_region(Region::USEast1, GroupId::new(1))
        .with_region(Region::APSoutheast2, GroupId::new(2));

    let us_group = directory
        .group_for_placement(&Placement::Region(Region::USEast1))
        .unwrap();
    let au_group = directory
        .group_for_placement(&Placement::Region(Region::APSoutheast2))
        .unwrap();

    assert_eq!(us_group, GroupId::new(1));
    assert_eq!(au_group, GroupId::new(2));
}

#[test]
fn unknown_region_returns_error() {
    let directory = Directory::new(GroupId::new(0)).with_region(Region::USEast1, GroupId::new(1));

    let result = directory.group_for_placement(&Placement::Region(Region::APSoutheast2));

    assert!(matches!(result, Err(DirectoryError::RegionNotFound(_))));
}

#[test]
fn custom_region_works() {
    let custom_region = Region::custom("eu-west-1");
    let directory =
        Directory::new(GroupId::new(0)).with_region(custom_region.clone(), GroupId::new(3));

    let group = directory
        .group_for_placement(&Placement::Region(custom_region))
        .unwrap();

    assert_eq!(group, GroupId::new(3));
}

#[test]
fn directory_builder_pattern() {
    // Test that builder pattern works fluently
    let directory = Directory::new(GroupId::new(0))
        .with_region(Region::USEast1, GroupId::new(1))
        .with_region(Region::APSoutheast2, GroupId::new(2))
        .with_region(Region::custom("eu-west-1"), GroupId::new(3));

    // All regions should be accessible
    assert!(
        directory
            .group_for_placement(&Placement::Region(Region::USEast1))
            .is_ok()
    );
    assert!(
        directory
            .group_for_placement(&Placement::Region(Region::APSoutheast2))
            .is_ok()
    );
    assert!(
        directory
            .group_for_placement(&Placement::Region(Region::custom("eu-west-1")))
            .is_ok()
    );
    assert!(directory.group_for_placement(&Placement::Global).is_ok());
}

// ============================================================================
// Property-Based Tests
// ============================================================================

use proptest::prelude::*;

proptest! {
    /// Property: Any regional placement with registered region returns a group
    #[test]
    fn prop_registered_region_always_routes(
        group_id in 0u64..1000u64,
        region_name in "[a-z]{2}-[a-z]{4,8}-[1-9]",
    ) {
        let region = Region::custom(region_name);
        let directory = Directory::new(GroupId::new(0))
            .with_region(region.clone(), GroupId::new(group_id));

        let result = directory.group_for_placement(&Placement::Region(region));
        prop_assert!(result.is_ok());
        prop_assert_eq!(result.unwrap(), GroupId::new(group_id));
    }

    /// Property: Global placement always succeeds
    #[test]
    fn prop_global_placement_always_succeeds(global_group_id in 0u64..1000u64) {
        let directory = Directory::new(GroupId::new(global_group_id));
        let result = directory.group_for_placement(&Placement::Global);

        prop_assert!(result.is_ok());
        prop_assert_eq!(result.unwrap(), GroupId::new(global_group_id));
    }

    /// Property: Unregistered region always fails
    #[test]
    fn prop_unregistered_region_always_fails(
        region1 in "[a-z]{2}-[a-z]{4,8}-1",
        region2 in "[a-z]{2}-[a-z]{4,8}-2",
    ) {
        // Ensure regions are different
        prop_assume!(region1 != region2);

        let directory = Directory::new(GroupId::new(0))
            .with_region(Region::custom(&region1), GroupId::new(1));

        let result = directory.group_for_placement(&Placement::Region(Region::custom(&region2)));
        prop_assert!(result.is_err());
    }

    /// Property: Multiple regions can coexist
    #[test]
    fn prop_multiple_regions_independent(
        regions in prop::collection::vec("[a-z]{2}-[a-z]{4,8}-[1-9]", 1..10),
    ) {
        // Build directory with all regions
        let mut directory = Directory::new(GroupId::new(0));
        for (idx, region_name) in regions.iter().enumerate() {
            directory = directory.with_region(
                Region::custom(region_name),
                GroupId::new((idx + 1) as u64)
            );
        }

        // Verify each region routes correctly
        for (idx, region_name) in regions.iter().enumerate() {
            let result = directory.group_for_placement(
                &Placement::Region(Region::custom(region_name))
            );
            prop_assert!(result.is_ok());
            prop_assert_eq!(result.unwrap(), GroupId::new((idx + 1) as u64));
        }
    }

    /// Property: Clone preserves routing
    #[test]
    fn prop_clone_preserves_routing(
        global_id in 0u64..1000u64,
        region_id in 0u64..1000u64,
        region_name in "[a-z]{2}-[a-z]{4,8}-[1-9]",
    ) {
        let original = Directory::new(GroupId::new(global_id))
            .with_region(Region::custom(&region_name), GroupId::new(region_id));

        let cloned = original.clone();

        // Both should route identically
        let placement = Placement::Region(Region::custom(&region_name));
        let original_result = original.group_for_placement(&placement).unwrap();
        let cloned_result = cloned.group_for_placement(&placement).unwrap();

        prop_assert_eq!(original_result, cloned_result);

        // Global should also match
        let global_original = original.group_for_placement(&Placement::Global).unwrap();
        let global_cloned = cloned.group_for_placement(&Placement::Global).unwrap();

        prop_assert_eq!(global_original, global_cloned);
    }
}

// ============================================================================
// Serde Round-Trip Tests (using bincode - supports non-string map keys)
// ============================================================================

#[test]
fn serde_roundtrip_empty_directory() {
    let original = Directory::new(GroupId::new(42));

    let serialized = postcard::to_allocvec(&original).unwrap();
    let deserialized: Directory = postcard::from_bytes(&serialized).unwrap();

    assert_eq!(original, deserialized);
}

#[test]
fn serde_roundtrip_with_regions() {
    let original = Directory::new(GroupId::new(0))
        .with_region(Region::USEast1, GroupId::new(1))
        .with_region(Region::APSoutheast2, GroupId::new(2))
        .with_region(Region::custom("eu-west-1"), GroupId::new(3));

    let serialized = postcard::to_allocvec(&original).unwrap();
    let deserialized: Directory = postcard::from_bytes(&serialized).unwrap();

    assert_eq!(original, deserialized);

    // Verify routing works after deserialization
    assert_eq!(
        deserialized
            .group_for_placement(&Placement::Global)
            .unwrap(),
        GroupId::new(0)
    );
    assert_eq!(
        deserialized
            .group_for_placement(&Placement::Region(Region::USEast1))
            .unwrap(),
        GroupId::new(1)
    );
}

#[test]
fn serde_preserves_routing_semantics() {
    let original = Directory::new(GroupId::new(100))
        .with_region(Region::custom("custom-1"), GroupId::new(200));

    let bytes = postcard::to_allocvec(&original).unwrap();
    let restored: Directory = postcard::from_bytes(&bytes).unwrap();

    // Test all routing still works
    assert_eq!(
        restored.group_for_placement(&Placement::Global).unwrap(),
        GroupId::new(100)
    );
    assert_eq!(
        restored
            .group_for_placement(&Placement::Region(Region::custom("custom-1")))
            .unwrap(),
        GroupId::new(200)
    );
    assert!(
        restored
            .group_for_placement(&Placement::Region(Region::custom("unknown")))
            .is_err()
    );
}

// ============================================================================
// Thread-Safety Tests
// ============================================================================

#[test]
fn concurrent_lookups_are_thread_safe() {
    let directory = Arc::new(
        Directory::new(GroupId::new(0))
            .with_region(Region::USEast1, GroupId::new(1))
            .with_region(Region::APSoutheast2, GroupId::new(2)),
    );

    let mut handles = vec![];

    // Spawn 10 threads all doing lookups
    for i in 0..10 {
        let dir = Arc::clone(&directory);
        let handle = thread::spawn(move || {
            for _ in 0..1000 {
                let region = if i % 2 == 0 {
                    Region::USEast1
                } else {
                    Region::APSoutheast2
                };

                let result = dir.group_for_placement(&Placement::Region(region));
                assert!(result.is_ok());
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn clone_in_different_threads() {
    let original = Directory::new(GroupId::new(0)).with_region(Region::USEast1, GroupId::new(1));

    let mut handles = vec![];

    for _ in 0..5 {
        let dir = original.clone();
        let handle = thread::spawn(move || {
            let result = dir.group_for_placement(&Placement::Region(Region::USEast1));
            assert_eq!(result.unwrap(), GroupId::new(1));
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

// ============================================================================
// Edge Case Tests
// ============================================================================

use test_case::test_case;

#[test]
fn duplicate_region_overwrites_previous() {
    let directory = Directory::new(GroupId::new(0))
        .with_region(Region::USEast1, GroupId::new(1))
        .with_region(Region::USEast1, GroupId::new(999)); // Overwrite

    let group = directory
        .group_for_placement(&Placement::Region(Region::USEast1))
        .unwrap();

    assert_eq!(group, GroupId::new(999), "Last registration should win");
}

#[test]
fn custom_region_case_sensitive() {
    let directory = Directory::new(GroupId::new(0))
        .with_region(Region::custom("us-EAST-1"), GroupId::new(1))
        .with_region(Region::custom("us-east-1"), GroupId::new(2));

    // Different cases are different regions
    assert_eq!(
        directory
            .group_for_placement(&Placement::Region(Region::custom("us-EAST-1")))
            .unwrap(),
        GroupId::new(1)
    );
    assert_eq!(
        directory
            .group_for_placement(&Placement::Region(Region::custom("us-east-1")))
            .unwrap(),
        GroupId::new(2)
    );
}

#[test]
fn empty_directory_only_routes_global() {
    let directory = Directory::new(GroupId::new(42));

    assert!(directory.group_for_placement(&Placement::Global).is_ok());
    assert!(
        directory
            .group_for_placement(&Placement::Region(Region::USEast1))
            .is_err()
    );
    assert!(
        directory
            .group_for_placement(&Placement::Region(Region::APSoutheast2))
            .is_err()
    );
}

#[test]
fn large_number_of_regions() {
    let mut directory = Directory::new(GroupId::new(0));

    // Add 100 regions
    for i in 1..=100 {
        directory = directory.with_region(Region::custom(format!("region-{i}")), GroupId::new(i));
    }

    // Verify all can be looked up
    for i in 1..=100 {
        let result = directory
            .group_for_placement(&Placement::Region(Region::custom(format!("region-{i}"))));
        assert_eq!(result.unwrap(), GroupId::new(i));
    }
}

#[test_case(Region::USEast1, GroupId::new(1); "us-east-1")]
#[test_case(Region::APSoutheast2, GroupId::new(2); "ap-southeast-2")]
fn standard_regions_route_correctly(region: Region, expected_group: GroupId) {
    let directory = Directory::new(GroupId::new(0))
        .with_region(Region::USEast1, GroupId::new(1))
        .with_region(Region::APSoutheast2, GroupId::new(2));

    let result = directory.group_for_placement(&Placement::Region(region));
    assert_eq!(result.unwrap(), expected_group);
}

#[test]
fn error_message_contains_region() {
    let directory = Directory::new(GroupId::new(0));
    let result = directory.group_for_placement(&Placement::Region(Region::custom("missing")));

    match result {
        Err(DirectoryError::RegionNotFound(region)) => {
            assert_eq!(region, Region::custom("missing"));
        }
        _ => panic!("Expected RegionNotFound error"),
    }
}

#[test]
fn directory_equality() {
    let dir1 = Directory::new(GroupId::new(0))
        .with_region(Region::USEast1, GroupId::new(1))
        .with_region(Region::APSoutheast2, GroupId::new(2));

    let dir2 = Directory::new(GroupId::new(0))
        .with_region(Region::APSoutheast2, GroupId::new(2))
        .with_region(Region::USEast1, GroupId::new(1));

    // Order of insertion shouldn't matter for equality
    assert_eq!(dir1, dir2);
}

#[test]
fn directory_inequality() {
    let dir1 = Directory::new(GroupId::new(0)).with_region(Region::USEast1, GroupId::new(1));

    let dir2 = Directory::new(GroupId::new(0)).with_region(Region::USEast1, GroupId::new(2));

    assert_ne!(
        dir1, dir2,
        "Different group IDs should make directories unequal"
    );

    let dir3 = Directory::new(GroupId::new(99)).with_region(Region::USEast1, GroupId::new(1));

    assert_ne!(
        dir1, dir3,
        "Different global groups should make directories unequal"
    );
}

#[test]
fn clone_independence() {
    let original = Directory::new(GroupId::new(0));
    let cloned = original.clone();

    // They should be equal
    assert_eq!(original, cloned);

    // Modifying clone doesn't affect original (builder returns new instance)
    let modified = cloned.with_region(Region::USEast1, GroupId::new(1));

    assert_ne!(original, modified);
    assert!(
        original
            .group_for_placement(&Placement::Region(Region::USEast1))
            .is_err()
    );
    assert!(
        modified
            .group_for_placement(&Placement::Region(Region::USEast1))
            .is_ok()
    );
}

// ============================================================================
// Shard Migration Tests
// ============================================================================

use crate::{MigrationPhase, ShardMigration, ShardRouter};

#[test]
fn shard_router_routes_via_directory_by_default() {
    let directory = Directory::new(GroupId::new(0))
        .with_region(Region::USEast1, GroupId::new(1));
    let router = ShardRouter::new(directory);

    let group = router
        .group_for_tenant(100, &Placement::Region(Region::USEast1))
        .unwrap();
    assert_eq!(group, GroupId::new(1));

    let group = router.group_for_tenant(100, &Placement::Global).unwrap();
    assert_eq!(group, GroupId::new(0));
}

#[test]
fn start_migration_creates_preparing_phase() {
    let directory = Directory::new(GroupId::new(0));
    let mut router = ShardRouter::new(directory);

    let migration = router
        .start_migration(1, GroupId::new(0), GroupId::new(1))
        .unwrap();

    assert_eq!(migration.tenant_id, 1);
    assert_eq!(migration.source_group, GroupId::new(0));
    assert_eq!(migration.destination_group, GroupId::new(1));
    assert_eq!(migration.phase, MigrationPhase::Preparing);
    assert_eq!(migration.records_copied, 0);
    assert_eq!(migration.total_records, 0);
}

#[test]
fn start_migration_rejects_same_group() {
    let directory = Directory::new(GroupId::new(0));
    let mut router = ShardRouter::new(directory);

    let result = router.start_migration(1, GroupId::new(5), GroupId::new(5));
    assert!(matches!(result, Err(DirectoryError::SameGroup(_))));
}

#[test]
fn start_migration_rejects_duplicate() {
    let directory = Directory::new(GroupId::new(0));
    let mut router = ShardRouter::new(directory);

    router
        .start_migration(1, GroupId::new(0), GroupId::new(1))
        .unwrap();

    let result = router.start_migration(1, GroupId::new(0), GroupId::new(2));
    assert!(matches!(
        result,
        Err(DirectoryError::MigrationInProgress(1))
    ));
}

#[test]
fn migration_reads_from_source_until_complete() {
    let directory = Directory::new(GroupId::new(0));
    let mut router = ShardRouter::new(directory);

    router
        .start_migration(1, GroupId::new(10), GroupId::new(20))
        .unwrap();

    // Preparing: reads from source
    let group = router.group_for_tenant(1, &Placement::Global).unwrap();
    assert_eq!(group, GroupId::new(10));

    // Copying: reads from source
    router.advance_migration(1).unwrap();
    let group = router.group_for_tenant(1, &Placement::Global).unwrap();
    assert_eq!(group, GroupId::new(10));

    // CatchUp: reads from source
    router.advance_migration(1).unwrap();
    let group = router.group_for_tenant(1, &Placement::Global).unwrap();
    assert_eq!(group, GroupId::new(10));

    // Complete: reads from destination
    router.advance_migration(1).unwrap();
    let group = router.group_for_tenant(1, &Placement::Global).unwrap();
    assert_eq!(group, GroupId::new(20));
}

#[test]
fn migration_dual_writes_during_copy_and_catchup() {
    let directory = Directory::new(GroupId::new(0));
    let mut router = ShardRouter::new(directory);

    router
        .start_migration(1, GroupId::new(10), GroupId::new(20))
        .unwrap();

    // Preparing: single write to source
    let groups = router
        .write_groups_for_tenant(1, &Placement::Global)
        .unwrap();
    assert_eq!(groups, vec![GroupId::new(10)]);

    // Copying: dual write
    router.advance_migration(1).unwrap();
    let groups = router
        .write_groups_for_tenant(1, &Placement::Global)
        .unwrap();
    assert_eq!(groups, vec![GroupId::new(10), GroupId::new(20)]);

    // CatchUp: dual write
    router.advance_migration(1).unwrap();
    let groups = router
        .write_groups_for_tenant(1, &Placement::Global)
        .unwrap();
    assert_eq!(groups, vec![GroupId::new(10), GroupId::new(20)]);

    // Complete: single write to destination
    router.advance_migration(1).unwrap();
    let groups = router
        .write_groups_for_tenant(1, &Placement::Global)
        .unwrap();
    assert_eq!(groups, vec![GroupId::new(20)]);
}

#[test]
fn advance_migration_through_all_phases() {
    let directory = Directory::new(GroupId::new(0));
    let mut router = ShardRouter::new(directory);

    router
        .start_migration(1, GroupId::new(0), GroupId::new(1))
        .unwrap();

    assert_eq!(
        router.advance_migration(1).unwrap(),
        MigrationPhase::Copying
    );
    assert_eq!(
        router.advance_migration(1).unwrap(),
        MigrationPhase::CatchUp
    );
    assert_eq!(
        router.advance_migration(1).unwrap(),
        MigrationPhase::Complete
    );
}

#[test]
fn completed_migration_sets_tenant_override() {
    let directory = Directory::new(GroupId::new(0));
    let mut router = ShardRouter::new(directory);

    router
        .start_migration(1, GroupId::new(0), GroupId::new(5))
        .unwrap();

    // Advance to complete
    router.advance_migration(1).unwrap(); // Copying
    router.advance_migration(1).unwrap(); // CatchUp
    router.advance_migration(1).unwrap(); // Complete

    // Advance again removes the migration entry
    router.advance_migration(1).unwrap();

    // Tenant override persists: routes to destination even without active migration
    let group = router.group_for_tenant(1, &Placement::Global).unwrap();
    assert_eq!(group, GroupId::new(5));
    assert_eq!(router.active_migration_count(), 0);
}

#[test]
fn advance_nonexistent_migration_errors() {
    let directory = Directory::new(GroupId::new(0));
    let mut router = ShardRouter::new(directory);

    let result = router.advance_migration(99);
    assert!(matches!(
        result,
        Err(DirectoryError::NoMigrationInProgress(99))
    ));
}

#[test]
fn update_progress_tracks_copy_state() {
    let directory = Directory::new(GroupId::new(0));
    let mut router = ShardRouter::new(directory);

    router
        .start_migration(1, GroupId::new(0), GroupId::new(1))
        .unwrap();
    router.advance_migration(1).unwrap(); // Copying

    router.update_progress(1, 500, 1000).unwrap();

    let migration = router.get_migration(1).unwrap();
    assert_eq!(migration.records_copied, 500);
    assert_eq!(migration.total_records, 1000);
    assert!((migration.progress_percent() - 50.0).abs() < f64::EPSILON);
}

#[test]
fn progress_percent_edge_cases() {
    // Zero total records in non-complete phase = 0%
    let migration = ShardMigration::new(1, GroupId::new(0), GroupId::new(1));
    assert!((migration.progress_percent() - 0.0).abs() < f64::EPSILON);

    // Zero total records in complete phase = 100%
    let mut complete = ShardMigration::new(1, GroupId::new(0), GroupId::new(1));
    complete.phase = MigrationPhase::Complete;
    assert!((complete.progress_percent() - 100.0).abs() < f64::EPSILON);

    // Capped at 100%
    let mut over = ShardMigration::new(1, GroupId::new(0), GroupId::new(1));
    over.records_copied = 1500;
    over.total_records = 1000;
    assert!((over.progress_percent() - 100.0).abs() < f64::EPSILON);
}

#[test]
fn requires_dual_write_only_during_copy_phases() {
    let mut migration = ShardMigration::new(1, GroupId::new(0), GroupId::new(1));

    assert!(!migration.requires_dual_write()); // Preparing

    migration.phase = MigrationPhase::Copying;
    assert!(migration.requires_dual_write());

    migration.phase = MigrationPhase::CatchUp;
    assert!(migration.requires_dual_write());

    migration.phase = MigrationPhase::Complete;
    assert!(!migration.requires_dual_write());
}

#[test]
fn multiple_tenant_migrations_independent() {
    let directory = Directory::new(GroupId::new(0));
    let mut router = ShardRouter::new(directory);

    router
        .start_migration(1, GroupId::new(0), GroupId::new(1))
        .unwrap();
    router
        .start_migration(2, GroupId::new(0), GroupId::new(2))
        .unwrap();

    assert_eq!(router.active_migration_count(), 2);

    // Advance tenant 1 only
    router.advance_migration(1).unwrap(); // Copying

    let m1 = router.get_migration(1).unwrap();
    assert_eq!(m1.phase, MigrationPhase::Copying);

    let m2 = router.get_migration(2).unwrap();
    assert_eq!(m2.phase, MigrationPhase::Preparing);
}

#[test]
fn active_migrations_returns_all() {
    let directory = Directory::new(GroupId::new(0));
    let mut router = ShardRouter::new(directory);

    router
        .start_migration(10, GroupId::new(0), GroupId::new(1))
        .unwrap();
    router
        .start_migration(20, GroupId::new(0), GroupId::new(2))
        .unwrap();

    let migrations = router.active_migrations();
    assert_eq!(migrations.len(), 2);
    assert!(migrations.contains_key(&10));
    assert!(migrations.contains_key(&20));
}
