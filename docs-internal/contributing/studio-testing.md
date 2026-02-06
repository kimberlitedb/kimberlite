# Kimberlite Studio - End-to-End Testing Guide

## Prerequisites

1. Build the project:
   ```bash
   just build
   ```

2. Start a Kimberlite instance with Studio:
   ```bash
   # Via CLI (once integrated with kmb dev)
   kmb dev --studio

   # Or directly via test harness
   cargo run -p kimberlite-studio --example standalone
   ```

## Manual Testing Workflow

### Test 1: Basic Studio Startup

**Objective**: Verify Studio starts and serves all assets correctly

**Steps**:
1. Navigate to `http://localhost:5555`
2. Open browser DevTools (Network tab)
3. Verify all assets load with 200 status:
   - `/` (HTML)
   - `/css/studio.css`
   - `/fonts/*.woff2` (all fonts)
   - `/vendor/datastar.js`
   - `/icons/sustyicons.svg`
4. Check console for errors (should be clean)

**Expected Result**:
- All assets load successfully
- No console errors
- Studio UI renders with header, sidebar, and main area
- Theme toggle works

**Pass/Fail**: [ ]

---

### Test 2: Tenant Selection

**Objective**: Verify tenant selector enforces compliance requirement

**Steps**:
1. Load Studio (default: no tenant selected)
2. Observe tenant selector shows "Select tenant..."
3. Verify warning message: "⚠️ Select a tenant to execute queries"
4. Verify Execute Query button is disabled
5. Select "dev-fixtures (ID: 1)" from dropdown
6. Verify warning disappears
7. Verify Execute Query button is enabled

**Expected Result**:
- Cannot execute queries without tenant selection
- Warning message is clear and visible
- Button state reflects selection state

**Pass/Fail**: [ ]

---

### Test 3: Query Execution (Mock Data)

**Objective**: Verify query editor and results display

**Steps**:
1. Select tenant 1
2. Enter query: `SELECT * FROM patients LIMIT 10`
3. Click "Execute Query" button
4. Observe loading state (button shows "Running...")
5. Verify results table appears after ~500ms
6. Check table has:
   - Headers: id, name, created_at
   - Rows: Alice, Bob (mock data)
   - Footer: "2 rows"

**Expected Result**:
- Loading state displays correctly
- Results render as styled table
- No errors in console

**Pass/Fail**: [ ]

---

### Test 4: Keyboard Shortcuts

**Objective**: Verify all keyboard shortcuts work

**Steps**:
1. Press `Cmd+K` (or `Ctrl+K`)
   - Verify query editor receives focus
2. Type a query: `SELECT 1`
3. Press `Ctrl+Enter` (or `Cmd+Enter`)
   - Verify query executes (same as clicking button)
4. Clear query, select no tenant, click Execute
   - Verify error message appears
5. Press `Escape`
   - Verify error message clears

**Expected Result**:
- All shortcuts work as documented
- Focus management is correct
- Error dismissal works

**Pass/Fail**: [ ]

---

### Test 5: Theme Toggle

**Objective**: Verify light/dark mode switching

**Steps**:
1. Click theme toggle button (🌙)
2. Observe page switches to dark mode
3. Verify colors invert correctly:
   - Background darkens
   - Text lightens
   - Borders remain visible
4. Click theme toggle again (☀️)
5. Verify page returns to light mode
6. Reload page
7. Verify theme persists (uses localStorage)

**Expected Result**:
- Theme toggle works smoothly
- All colors adapt via `light-dark()` function
- Theme preference persists across reloads

**Pass/Fail**: [ ]

---

### Test 6: Responsive Layout

**Objective**: Verify mobile and narrow viewport support

**Steps**:
1. Open DevTools responsive design mode
2. Set viewport to 320px width (iPhone SE)
3. Verify:
   - Sidebar collapses/hides
   - Query editor remains usable
   - Table scrolls horizontally
   - No horizontal page scroll
4. Set viewport to 768px (tablet)
5. Verify sidebar reappears
6. Set viewport to 1920px (desktop)
7. Verify max-width wrapper contains content

**Expected Result**:
- Layout adapts gracefully to all viewport sizes
- Content remains accessible and usable
- No layout breaking or overflow

**Pass/Fail**: [ ]

---

### Test 7: Time-Travel Controls (Mock)

**Objective**: Verify time-travel UI appears and functions

**Steps**:
1. Execute a query (to set mock max_offset)
2. Observe "Query at offset" section appears
3. Move slider to different positions
4. Verify offset value updates in real-time
5. Click "← Latest" button
6. Verify slider resets to null (latest)

**Expected Result**:
- Time-travel controls only show when max_offset > 0
- Slider is interactive and updates signal
- Latest button resets to current state

**Pass/Fail**: [ ]

---

### Test 8: Schema Tree Sidebar

**Objective**: Verify schema navigation

**Steps**:
1. No tenant selected: verify message "Select a tenant to view schema"
2. Select tenant 1
3. Observe schema tree populates (mock data):
   - Tenant node: "tenant-1 (ID: 1)"
   - Table nodes: patients, visits
   - Column nodes: nested under tables
4. Verify hierarchical indentation (data-level attributes)
5. Hover over table names
6. Verify hover style applies

**Expected Result**:
- Schema tree shows clear hierarchy
- Empty state message when no tenant
- Interactive hover states

**Pass/Fail**: [ ]

---

### Test 9: Error Handling

**Objective**: Verify error messages display correctly

**Steps**:
1. Select no tenant
2. Click Execute Query
3. Verify error banner appears:
   - Title: "Error"
   - Message: "Please select a tenant first"
4. Verify error has red/warning styling
5. Press Escape
6. Verify error dismisses
7. Enter invalid SQL (future: when real execution works)
8. Verify SQL error displays with details

**Expected Result**:
- Errors are clearly visible
- Error styling is distinct
- Errors can be dismissed
- Error messages are helpful

**Pass/Fail**: [ ]

---

### Test 10: Accessibility Audit

**Objective**: Verify WCAG 2.1 Level AA compliance

**Steps**:
1. Run Lighthouse accessibility audit:
   ```bash
   # Chrome DevTools > Lighthouse > Accessibility
   ```
2. Verify score ≥ 90 (target: 100)
3. Test keyboard navigation:
   - Tab through all interactive elements
   - Verify focus indicators are visible
   - Verify no keyboard traps
4. Test with screen reader (VoiceOver/NVDA):
   - Verify all labels are read
   - Verify live regions announce updates
   - Verify ARIA labels are correct
5. Check color contrast:
   ```bash
   # Use axe DevTools or manual checker
   ```
6. Verify reduced motion support:
   - Enable "Reduce motion" in OS settings
   - Reload page
   - Verify animations are minimal

**Expected Result**:
- Lighthouse score ≥ 90
- All keyboard navigation works
- Screen reader reads all content correctly
- No color contrast failures
- Reduced motion is respected

**Pass/Fail**: [ ]

---

### Test 11: SSE Connection (Mock Events)

**Objective**: Verify Server-Sent Events work

**Steps**:
1. Open Network tab in DevTools
2. Filter for "EventStream" or "/sse/"
3. Verify connection to `/sse/projection-updates`
4. Connection should:
   - Status: 200
   - Type: eventsource
   - Stay open (long-lived connection)
5. In console, trigger a mock projection event:
   ```javascript
   // Simulate event via Datastar
   console.log('SSE connection active');
   ```
6. Verify keep-alive pings every 15 seconds

**Expected Result**:
- SSE connection establishes successfully
- Connection remains open
- No reconnection loops
- Events stream without errors

**Pass/Fail**: [ ]

---

### Test 12: Performance Benchmarks

**Objective**: Verify performance meets targets

**Steps**:
1. Run Lighthouse performance audit
2. Verify metrics:
   - First Contentful Paint < 1.0s
   - Time to Interactive < 2.0s
   - Total Blocking Time < 300ms
3. Check bundle size:
   - Main CSS: < 100KB (uncompressed)
   - Fonts: ~1.2MB (acceptable for quality fonts)
   - Datastar: ~30KB
4. Verify font loading:
   - Critical fonts preloaded
   - No FOUT (flash of unstyled text)
5. Check JavaScript bundle:
   - Only Datastar.js loaded
   - No build step required

**Expected Result**:
- Performance score ≥ 90
- Fast initial load
- Minimal blocking resources
- Fonts load smoothly

**Pass/Fail**: [ ]

---

## Automated Testing

### Unit Tests

```bash
# Run Studio unit tests
cargo test -p kimberlite-studio

# Run with coverage
cargo tarpaulin -p kimberlite-studio
```

**Expected**: All tests pass, coverage ≥ 80%

### Integration Tests

```bash
# Run integration tests (when implemented)
cargo test -p kimberlite-studio --test integration

# Test SSE endpoints
cargo test -p kimberlite-studio --test sse_integration
```

### End-to-End Tests (Future)

```bash
# Using Playwright or similar
npm run test:e2e

# Or via Rust with headless_chrome
cargo test -p kimberlite-studio --test e2e
```

## CI/CD Checklist

- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] Lighthouse scores ≥ 90 (performance, accessibility, best practices)
- [ ] No console errors in test run
- [ ] Asset sizes within limits
- [ ] Build completes in < 2 minutes

## Known Limitations (Current Mock Implementation)

1. **Query Execution**: Returns mock data, not real database queries
2. **Schema Tree**: Shows hardcoded mock schema
3. **Projection Events**: Not yet connected to real Kimberlite events
4. **Time-Travel**: UI exists but doesn't query at specific offsets yet
5. **Tenant Management**: Mock tenant list

**Next Steps**: Wire up real kimberlite_client integration to replace mocks.

## Bug Reporting

If you find issues during testing:

1. Note the test case number
2. Record browser/OS version
3. Capture console errors
4. Take screenshots/screen recordings
5. File issue with label `kimberlite-studio`

## Success Criteria

✅ All manual tests pass
✅ Lighthouse accessibility ≥ 90
✅ Lighthouse performance ≥ 90
✅ Zero console errors
✅ Works in Chrome, Firefox, Safari
✅ Works on mobile viewports (320px+)
✅ Keyboard navigation complete
✅ Screen reader compatible
