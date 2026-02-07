package kimberlite

import "time"

// DataClass represents the classification level of data.
type DataClass int

const (
	// DataClassPublic is non-sensitive public data.
	DataClassPublic DataClass = iota
	// DataClassInternal is organization-internal data.
	DataClassInternal
	// DataClassConfidential is sensitive confidential data.
	DataClassConfidential
	// DataClassRestricted is highly restricted data (e.g., PHI/PII).
	DataClassRestricted
)

// String returns the string representation of a DataClass.
func (d DataClass) String() string {
	switch d {
	case DataClassPublic:
		return "public"
	case DataClassInternal:
		return "internal"
	case DataClassConfidential:
		return "confidential"
	case DataClassRestricted:
		return "restricted"
	default:
		return "unknown"
	}
}

// StreamID uniquely identifies a stream within a tenant.
type StreamID uint64

// Offset represents a position in the append-only log.
type Offset uint64

// TenantID uniquely identifies a tenant.
type TenantID uint64

// Value represents a database value. It can hold any supported type.
type Value struct {
	// Type is the underlying type descriptor.
	Type ValueType
	// Raw holds the value. Use the typed accessor methods.
	raw any
}

// ValueType enumerates the types a Value can hold.
type ValueType int

const (
	ValueTypeNull ValueType = iota
	ValueTypeInteger
	ValueTypeFloat
	ValueTypeText
	ValueTypeBoolean
	ValueTypeBytes
	ValueTypeTimestamp
)

// IsNull returns true if the value is NULL.
func (v Value) IsNull() bool { return v.Type == ValueTypeNull }

// AsInt returns the value as int64, or 0 if not an integer.
func (v Value) AsInt() int64 {
	if v.Type == ValueTypeInteger {
		if n, ok := v.raw.(int64); ok {
			return n
		}
	}
	return 0
}

// AsFloat returns the value as float64, or 0 if not a float.
func (v Value) AsFloat() float64 {
	if v.Type == ValueTypeFloat {
		if f, ok := v.raw.(float64); ok {
			return f
		}
	}
	return 0
}

// AsText returns the value as string, or "" if not text.
func (v Value) AsText() string {
	if v.Type == ValueTypeText {
		if s, ok := v.raw.(string); ok {
			return s
		}
	}
	return ""
}

// AsBool returns the value as bool, or false if not boolean.
func (v Value) AsBool() bool {
	if v.Type == ValueTypeBoolean {
		if b, ok := v.raw.(bool); ok {
			return b
		}
	}
	return false
}

// AsBytes returns the value as []byte, or nil if not bytes.
func (v Value) AsBytes() []byte {
	if v.Type == ValueTypeBytes {
		if b, ok := v.raw.([]byte); ok {
			return b
		}
	}
	return nil
}

// AsTimestamp returns the value as time.Time, or zero time if not a timestamp.
func (v Value) AsTimestamp() time.Time {
	if v.Type == ValueTypeTimestamp {
		if t, ok := v.raw.(time.Time); ok {
			return t
		}
	}
	return time.Time{}
}

// NewNull creates a NULL value.
func NewNull() Value { return Value{Type: ValueTypeNull} }

// NewInt creates an integer value.
func NewInt(n int64) Value { return Value{Type: ValueTypeInteger, raw: n} }

// NewFloat creates a float value.
func NewFloat(f float64) Value { return Value{Type: ValueTypeFloat, raw: f} }

// NewText creates a text value.
func NewText(s string) Value { return Value{Type: ValueTypeText, raw: s} }

// NewBool creates a boolean value.
func NewBool(b bool) Value { return Value{Type: ValueTypeBoolean, raw: b} }

// NewBytes creates a bytes value.
func NewBytes(b []byte) Value { return Value{Type: ValueTypeBytes, raw: b} }

// NewTimestamp creates a timestamp value.
func NewTimestamp(t time.Time) Value { return Value{Type: ValueTypeTimestamp, raw: t} }

// QueryResult holds the result of a SQL query.
type QueryResult struct {
	// Columns contains the column names in order.
	Columns []string
	// Rows contains the result data, each row mapping column name to value.
	Rows []map[string]Value
	// RowsAffected is the number of rows affected by a write operation.
	RowsAffected int64
}

// StreamInfo describes a stream in the database.
type StreamInfo struct {
	// ID is the stream identifier.
	ID StreamID
	// Name is the human-readable stream name.
	Name string
	// DataClass is the data classification level.
	DataClass DataClass
	// CreatedAt is when the stream was created.
	CreatedAt time.Time
}

// Event represents an event in a stream.
type Event struct {
	// Offset is the event's position in the log.
	Offset Offset
	// StreamID is the stream this event belongs to.
	StreamID StreamID
	// Data is the event payload.
	Data []byte
	// Timestamp is when the event was written.
	Timestamp time.Time
}
