package kimberlite

import (
	"testing"
	"time"
)

func TestVersion(t *testing.T) {
	if Version == "" {
		t.Fatal("Version should not be empty")
	}
}

func TestDataClassString(t *testing.T) {
	tests := []struct {
		class    DataClass
		expected string
	}{
		{DataClassPublic, "public"},
		{DataClassInternal, "internal"},
		{DataClassConfidential, "confidential"},
		{DataClassRestricted, "restricted"},
		{DataClass(99), "unknown"},
	}

	for _, tt := range tests {
		got := tt.class.String()
		if got != tt.expected {
			t.Errorf("DataClass(%d).String() = %q, want %q", tt.class, got, tt.expected)
		}
	}
}

func TestValueNull(t *testing.T) {
	v := NewNull()
	if !v.IsNull() {
		t.Fatal("NewNull().IsNull() should be true")
	}
	if v.AsInt() != 0 {
		t.Fatal("NULL AsInt should return 0")
	}
	if v.AsText() != "" {
		t.Fatal("NULL AsText should return empty string")
	}
}

func TestValueInt(t *testing.T) {
	v := NewInt(42)
	if v.IsNull() {
		t.Fatal("NewInt should not be null")
	}
	if v.AsInt() != 42 {
		t.Fatalf("AsInt() = %d, want 42", v.AsInt())
	}
	if v.AsText() != "" {
		t.Fatal("int AsText should return empty string")
	}
}

func TestValueFloat(t *testing.T) {
	v := NewFloat(3.14)
	if v.AsFloat() != 3.14 {
		t.Fatalf("AsFloat() = %f, want 3.14", v.AsFloat())
	}
}

func TestValueText(t *testing.T) {
	v := NewText("hello")
	if v.AsText() != "hello" {
		t.Fatalf("AsText() = %q, want %q", v.AsText(), "hello")
	}
}

func TestValueBool(t *testing.T) {
	v := NewBool(true)
	if !v.AsBool() {
		t.Fatal("AsBool() should be true")
	}

	v2 := NewBool(false)
	if v2.AsBool() {
		t.Fatal("AsBool() should be false")
	}
}

func TestValueBytes(t *testing.T) {
	data := []byte{0xDE, 0xAD, 0xBE, 0xEF}
	v := NewBytes(data)
	got := v.AsBytes()
	if len(got) != 4 || got[0] != 0xDE {
		t.Fatalf("AsBytes() unexpected result: %v", got)
	}
}

func TestValueTimestamp(t *testing.T) {
	now := time.Now()
	v := NewTimestamp(now)
	got := v.AsTimestamp()
	if !got.Equal(now) {
		t.Fatalf("AsTimestamp() = %v, want %v", got, now)
	}
}

func TestKimberliteError(t *testing.T) {
	err := &KimberliteError{
		Code:    "AUTH_FAILED",
		Message: "invalid token",
	}
	expected := "kimberlite [AUTH_FAILED]: invalid token"
	if err.Error() != expected {
		t.Fatalf("Error() = %q, want %q", err.Error(), expected)
	}

	err2 := &KimberliteError{
		Message: "connection refused",
	}
	expected2 := "kimberlite: connection refused"
	if err2.Error() != expected2 {
		t.Fatalf("Error() = %q, want %q", err2.Error(), expected2)
	}
}

func TestConnectRequiresTenant(t *testing.T) {
	// Connecting without a tenant should fail.
	// Note: This test doesn't actually attempt a real connection since
	// the FFI library isn't available in unit tests. It tests the
	// parameter validation layer.
	_, err := Connect("127.0.0.1:5432")
	if err == nil {
		t.Fatal("Connect without tenant should return error")
	}
	if err != ErrTenantRequired {
		t.Fatalf("expected ErrTenantRequired, got: %v", err)
	}
}

func TestQueryResult(t *testing.T) {
	result := QueryResult{
		Columns: []string{"id", "name"},
		Rows: []map[string]Value{
			{"id": NewInt(1), "name": NewText("Alice")},
			{"id": NewInt(2), "name": NewText("Bob")},
		},
		RowsAffected: 0,
	}

	if len(result.Columns) != 2 {
		t.Fatalf("expected 2 columns, got %d", len(result.Columns))
	}
	if len(result.Rows) != 2 {
		t.Fatalf("expected 2 rows, got %d", len(result.Rows))
	}
	if result.Rows[0]["name"].AsText() != "Alice" {
		t.Fatal("expected Alice in first row")
	}
}
