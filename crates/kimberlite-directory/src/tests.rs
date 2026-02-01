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

    let serialized = bincode::serialize(&original).unwrap();
    let deserialized: Directory = bincode::deserialize(&serialized).unwrap();

    assert_eq!(original, deserialized);
}

#[test]
fn serde_roundtrip_with_regions() {
    let original = Directory::new(GroupId::new(0))
        .with_region(Region::USEast1, GroupId::new(1))
        .with_region(Region::APSoutheast2, GroupId::new(2))
        .with_region(Region::custom("eu-west-1"), GroupId::new(3));

    let serialized = bincode::serialize(&original).unwrap();
    let deserialized: Directory = bincode::deserialize(&serialized).unwrap();

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

    let bytes = bincode::serialize(&original).unwrap();
    let restored: Directory = bincode::deserialize(&bytes).unwrap();

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
