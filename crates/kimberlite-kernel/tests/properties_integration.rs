// Integration test for property registry with kernel
use kimberlite_kernel::command::Command;
use kimberlite_kernel::{State, apply_committed};
use kimberlite_types::{DataClass, Placement, StreamId, StreamName};

#[test]
fn registry_records_kernel_annotations() {
    kimberlite_properties::registry::reset();

    let state = State::new();
    let cmd = Command::CreateStream {
        stream_id: StreamId::new(1),
        stream_name: StreamName::new("test".to_string()),
        data_class: DataClass::Public,
        placement: Placement::Global,
    };
    let _ = apply_committed(state, cmd).unwrap();

    let snap = kimberlite_properties::registry::snapshot();
    println!("Registry size: {}", snap.len());
    for (id, rec) in &snap {
        println!(
            "  {id} (evaluations: {}, violations: {})",
            rec.evaluations, rec.violations
        );
    }

    // Should have at least: kernel.stream_exists_after_create, kernel.stream_zero_offset_after_create
    assert!(
        snap.contains_key("kernel.stream_exists_after_create"),
        "Expected kernel.stream_exists_after_create to be recorded"
    );
}
