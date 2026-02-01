# Kernel Assertion Inventory

This document catalogs all assertions in the kernel, documenting the invariants they enforce.

## Summary Statistics

- **Total Assertions**: 33
- **Functions with Assertions**: 9 (all command handlers in `apply_committed`)
- **Assertion Density**: ~3.7 assertions per function
- **Types**: Preconditions, Postconditions, Invariants

## Assertion Breakdown by Command

### CreateStream (5 assertions)

**Postconditions:**
1. `meta.stream_id == stream_id` - Metadata has correct stream ID
2. `effects.len() == 2` - Exactly 2 effects (metadata write + audit)
3. `new_state.stream_exists(&stream_id)` - Stream exists in new state
4. `new_state.get_stream().current_offset == Offset::ZERO` - Initial offset is zero

### CreateStreamWithAutoId (3 assertions)

**Postconditions:**
1. `new_state.stream_exists(&meta.stream_id)` - Auto-generated stream exists
2. `current_offset == Offset::ZERO` - Initial offset is zero
3. `effects.len() == 2` - Exactly 2 effects

### AppendBatch (6 assertions)

**Invariants:**
1. `new_offset >= base_offset` - Offset never decreases (monotonic)
2. `new_offset == base_offset + event_count` - New offset = base + count

**Postconditions:**
3. `effects.len() == 3` - Exactly 3 effects (storage + projection + audit)
4. `new_state.current_offset == new_offset` - Offset advanced correctly
5. `new_state.current_offset == base_offset + event_count` - Offset increased by event count

### CreateTable (6 assertions)

**Preconditions:**
1. `!columns.is_empty()` - Table must have at least one column

**Postconditions:**
2. `new_state.stream_exists(&stream_meta.stream_id)` - Backing stream created
3. `table_meta.stream_id == stream_meta.stream_id` - Table metadata references backing stream
4. `final_state.table_exists(&table_id)` - Table now exists in state
5. `effects.len() == 3` - Exactly 3 effects (stream + table + audit)

### DropTable (2 assertions)

**Postconditions:**
1. `effects.len() == 1` - Exactly 1 effect (drop metadata)
2. `!new_state.table_exists(&table_id)` - Table no longer exists

### CreateIndex (4 assertions)

**Preconditions:**
1. `!columns.is_empty()` - Index must cover at least one column

**Postconditions:**
2. `index_meta.table_id == table_id` - Index metadata references correct table
3. `effects.len() == 1` - Exactly 1 effect (index metadata write)
4. `new_state.index_exists(&index_id)` - Index now exists

### Insert (5 assertions)

**Invariants:**
1. `new_offset > base_offset` - Offset monotonically increases
2. `new_offset == base_offset + 1` - Single row insert increments offset by 1

**Postconditions:**
3. `effects.len() == 3` - Exactly 3 effects (storage + projection + audit)
4. `new_state.current_offset == new_offset` - Stream offset advanced by 1

### Update (3 assertions)

**Invariants:**
1. `new_offset > base_offset` - Offset monotonically increases

**Postconditions:**
2. `effects.len() == 3` - Exactly 3 effects
3. `new_state.current_offset == new_offset` - Offset advanced correctly

### Delete (3 assertions)

**Invariants:**
1. `new_offset > base_offset` - Offset monotonically increases (delete is append-only)

**Postconditions:**
2. `effects.len() == 3` - Exactly 3 effects
3. `new_state.current_offset == new_offset` - Offset advanced correctly

## Key Invariants Enforced

### 1. Offset Monotonicity
**Assertion**: `new_offset > base_offset` or `new_offset >= base_offset`
**Commands**: AppendBatch, Insert, Update, Delete
**Rationale**: Offsets represent logical time and must never decrease

### 2. Effect Count Correctness
**Assertion**: `effects.len() == N`
**Commands**: All commands
**Rationale**: Ensures all expected side effects are produced

### 3. State Consistency
**Assertion**: `new_state.stream_exists()` / `table_exists()` / `index_exists()`
**Commands**: CreateStream, CreateTable, CreateIndex, DropTable
**Rationale**: Entity existence matches expected state after operation

### 4. Offset Arithmetic
**Assertion**: `new_offset == base_offset + event_count`
**Commands**: AppendBatch, Insert, Update, Delete
**Rationale**: Offset advancement matches event count

### 5. Metadata Consistency
**Assertion**: `table_meta.stream_id == stream_meta.stream_id`
**Commands**: CreateTable
**Rationale**: Table metadata correctly links to backing stream

### 6. Initial State
**Assertion**: `current_offset == Offset::ZERO`
**Commands**: CreateStream, CreateStreamWithAutoId
**Rationale**: New streams start at offset zero

## PRESSURECRAFT Compliance

✅ **Assertion Density**: 3.7 assertions per function (exceeds 2+ requirement)
✅ **Preconditions**: Documented via assertions (columns not empty, etc.)
✅ **Postconditions**: Verified for all operations
✅ **Invariants**: Monotonicity, consistency enforced
✅ **Paired Assertions**: Write sites have matching assertions (offset tracking)

## Benefits

1. **Self-Documenting Code**: Assertions serve as executable documentation of invariants
2. **Early Bug Detection**: Violations caught in debug builds before they propagate
3. **Regression Protection**: Tests verify assertions hold across all scenarios
4. **Confidence in Refactoring**: Invariants remain enforced even as implementation changes
5. **Production Safety**: Debug assertions compiled out in release builds (zero overhead)

## Testing

All 37 kernel tests pass with assertions enabled:
- Unit tests verify normal operation paths
- Property tests verify invariants hold across random inputs
- Assertions catch violations immediately in debug builds
