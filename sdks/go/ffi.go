package kimberlite

// CGo bindings to libkimberlite_ffi (handle-based API).
//
// The FFI library is built from the kimberlite-ffi crate and must be
// available at link time. Set CGO_LDFLAGS to point to the library:
//
//	CGO_LDFLAGS="-L/path/to/target/release -lkimberlite_ffi" go build

/*
#cgo LDFLAGS: -lkimberlite_ffi
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

// KmbError codes returned by all FFI functions.
typedef enum {
	KMB_OK                    = 0,
	KMB_ERR_NULL_POINTER      = 1,
	KMB_ERR_INVALID_UTF8      = 2,
	KMB_ERR_CONNECTION_FAILED = 3,
	KMB_ERR_STREAM_NOT_FOUND  = 4,
	KMB_ERR_PERMISSION_DENIED = 5,
	KMB_ERR_INVALID_DATA_CLASS = 6,
	KMB_ERR_OFFSET_OUT_OF_RANGE = 7,
	KMB_ERR_QUERY_SYNTAX      = 8,
	KMB_ERR_QUERY_EXECUTION   = 9,
	KMB_ERR_TENANT_NOT_FOUND  = 10,
	KMB_ERR_AUTH_FAILED       = 11,
	KMB_ERR_TIMEOUT           = 12,
	KMB_ERR_INTERNAL          = 13,
	KMB_ERR_CLUSTER_UNAVAILABLE = 14,
	KMB_ERR_UNKNOWN           = 15,
} KmbError;

// Opaque client handle.
typedef struct KmbClient KmbClient;

// Client connection configuration.
typedef struct {
	const char** addresses;
	size_t       address_count;
	uint64_t     tenant_id;
	const char*  auth_token;
	const char*  client_name;
	const char*  client_version;
} KmbClientConfig;

// Result from read_events.
typedef struct {
	uint8_t** events;
	size_t*   event_lengths;
	size_t    event_count;
} KmbReadResult;

// Query value types.
#define KMB_VALUE_NULL      0
#define KMB_VALUE_BIGINT    1
#define KMB_VALUE_TEXT      2
#define KMB_VALUE_BOOLEAN   3
#define KMB_VALUE_TIMESTAMP 4

// A single value in a query result row.
typedef struct {
	int     value_type;
	int64_t bigint_val;
	char*   text_val;
	int     bool_val;
	int64_t timestamp_val;
} KmbQueryValue;

// A complete query result (2-D array of values).
typedef struct {
	char**          columns;
	size_t          column_count;
	KmbQueryValue** rows;
	size_t*         row_lengths;
	size_t          row_count;
} KmbQueryResult;

// FFI function declarations.
extern KmbError    kmb_client_connect(const KmbClientConfig* config, KmbClient** client_out);
extern void        kmb_client_disconnect(KmbClient* client);
extern KmbError    kmb_client_create_stream(KmbClient* client, const char* name, int data_class, uint64_t* stream_id_out);
extern KmbError    kmb_client_append(KmbClient* client, uint64_t stream_id, uint64_t expected_offset, const uint8_t** events, const size_t* event_lengths, size_t event_count, uint64_t* first_offset_out);
extern KmbError    kmb_client_read_events(KmbClient* client, uint64_t stream_id, uint64_t from_offset, uint64_t max_bytes, KmbReadResult** result_out);
extern void        kmb_read_result_free(KmbReadResult* result);
extern KmbError    kmb_client_query(KmbClient* client, const char* sql, const void* params, size_t param_count, KmbQueryResult** result_out);
extern void        kmb_query_result_free(KmbQueryResult* result);
extern const char* kmb_error_message(KmbError error);

// kmb_connect_helper avoids the CGo pointer-in-pointer restriction by
// building KmbClientConfig entirely on the C stack (all pointer fields
// are C-allocated strings, not Go pointers).
static KmbError kmb_connect_helper(
	const char* addr,
	uint64_t    tenant_id,
	const char* auth_token,
	const char* client_name,
	const char* client_version,
	KmbClient** client_out
) {
	const char* addrs[1];
	addrs[0] = addr;

	KmbClientConfig cfg;
	memset(&cfg, 0, sizeof(cfg));
	cfg.addresses      = addrs;
	cfg.address_count  = 1;
	cfg.tenant_id      = tenant_id;
	cfg.auth_token     = auth_token;
	cfg.client_name    = client_name;
	cfg.client_version = client_version;

	return kmb_client_connect(&cfg, client_out);
}
*/
import "C"

import (
	"fmt"
	"time"
	"unsafe"
)

// ffiAvailable returns true if the CGo FFI library is linked.
func ffiAvailable() bool {
	return true
}

// ffiConnect connects to the server and returns an opaque client handle.
func ffiConnect(addr string, tenantID uint64, token string) (unsafe.Pointer, error) {
	cAddr := C.CString(addr)
	defer C.free(unsafe.Pointer(cAddr))

	cClientName := C.CString("kimberlite-go")
	defer C.free(unsafe.Pointer(cClientName))

	cClientVersion := C.CString(Version)
	defer C.free(unsafe.Pointer(cClientVersion))

	var cToken *C.char
	if token != "" {
		cToken = C.CString(token)
		defer C.free(unsafe.Pointer(cToken))
	}

	var clientOut *C.KmbClient
	rc := C.kmb_connect_helper(cAddr, C.uint64_t(tenantID), cToken, cClientName, cClientVersion, &clientOut)
	if rc != C.KMB_OK {
		return nil, mapFFIError(rc)
	}
	return unsafe.Pointer(clientOut), nil
}

// ffiDisconnect disconnects and frees the client handle.
func ffiDisconnect(handle unsafe.Pointer) error {
	if handle == nil {
		return nil
	}
	C.kmb_client_disconnect((*C.KmbClient)(handle))
	return nil
}

// ffiQuery executes a SQL query and returns the results.
func ffiQuery(handle unsafe.Pointer, sql string) (*QueryResult, error) {
	if handle == nil {
		return nil, ErrNotConnected
	}

	cSQL := C.CString(sql)
	defer C.free(unsafe.Pointer(cSQL))

	var resultOut *C.KmbQueryResult
	rc := C.kmb_client_query((*C.KmbClient)(handle), cSQL, nil, 0, &resultOut)
	if rc != C.KMB_OK {
		return nil, mapFFIError(rc)
	}
	defer C.kmb_query_result_free(resultOut)

	return convertQueryResult(resultOut), nil
}

// ffiCreateStream creates a new stream and returns its info.
func ffiCreateStream(handle unsafe.Pointer, name string, class DataClass) (*StreamInfo, error) {
	if handle == nil {
		return nil, ErrNotConnected
	}

	cName := C.CString(name)
	defer C.free(unsafe.Pointer(cName))

	var streamIDOut C.uint64_t
	rc := C.kmb_client_create_stream((*C.KmbClient)(handle), cName, C.int(class), &streamIDOut)
	if rc != C.KMB_OK {
		return nil, mapFFIError(rc)
	}

	return &StreamInfo{
		ID:        StreamID(streamIDOut),
		Name:      name,
		DataClass: class,
		CreatedAt: time.Now(),
	}, nil
}

// ffiAppend appends events to a stream.
func ffiAppend(handle unsafe.Pointer, streamID uint64, events [][]byte) (Offset, error) {
	if handle == nil {
		return 0, ErrNotConnected
	}
	if len(events) == 0 {
		return 0, nil
	}

	n := len(events)
	ptrSize := C.size_t(unsafe.Sizeof((*C.uint8_t)(nil)))
	lenSize := C.size_t(unsafe.Sizeof(C.size_t(0)))

	// Allocate C arrays for event pointers and lengths to avoid the CGo
	// pointer-in-pointer restriction (Go slice of C pointers → C memory).
	cEventPtrs := (**C.uint8_t)(C.malloc(ptrSize * C.size_t(n)))
	defer C.free(unsafe.Pointer(cEventPtrs))
	cEventLens := (*C.size_t)(C.malloc(lenSize * C.size_t(n)))
	defer C.free(unsafe.Pointer(cEventLens))

	ptrSlice := (*[1 << 20]*C.uint8_t)(unsafe.Pointer(cEventPtrs))[:n:n]
	lenSlice := (*[1 << 20]C.size_t)(unsafe.Pointer(cEventLens))[:n:n]

	for i, evt := range events {
		p := C.CBytes(evt) // copies to C heap
		defer C.free(p)
		ptrSlice[i] = (*C.uint8_t)(p)
		lenSlice[i] = C.size_t(len(evt))
	}

	var firstOffsetOut C.uint64_t
	rc := C.kmb_client_append(
		(*C.KmbClient)(handle),
		C.uint64_t(streamID),
		0, // expected_offset: 0 = no optimistic concurrency check
		cEventPtrs,
		cEventLens,
		C.size_t(n),
		&firstOffsetOut,
	)
	if rc != C.KMB_OK {
		return 0, mapFFIError(rc)
	}
	return Offset(firstOffsetOut), nil
}

// ffiReadEvents reads events from a stream starting at fromOffset.
func ffiReadEvents(handle unsafe.Pointer, streamID, fromOffset, maxBytes uint64) ([]Event, error) {
	if handle == nil {
		return nil, ErrNotConnected
	}

	var resultOut *C.KmbReadResult
	rc := C.kmb_client_read_events(
		(*C.KmbClient)(handle),
		C.uint64_t(streamID),
		C.uint64_t(fromOffset),
		C.uint64_t(maxBytes),
		&resultOut,
	)
	if rc != C.KMB_OK {
		return nil, mapFFIError(rc)
	}
	defer C.kmb_read_result_free(resultOut)

	n := int(resultOut.event_count)
	if n == 0 {
		return nil, nil
	}

	evPtrs := (*[1 << 20]*C.uint8_t)(unsafe.Pointer(resultOut.events))[:n:n]
	evLens := (*[1 << 20]C.size_t)(unsafe.Pointer(resultOut.event_lengths))[:n:n]

	out := make([]Event, n)
	for i := range out {
		dataLen := int(evLens[i])
		var data []byte
		if dataLen > 0 && evPtrs[i] != nil {
			data = C.GoBytes(unsafe.Pointer(evPtrs[i]), C.int(dataLen))
		}
		out[i] = Event{
			Offset:    Offset(fromOffset) + Offset(i),
			StreamID:  StreamID(streamID),
			Data:      data,
			Timestamp: time.Now(),
		}
	}
	return out, nil
}

// mapFFIError converts a KmbError code to a Go error.
func mapFFIError(rc C.KmbError) error {
	msg := C.GoString(C.kmb_error_message(rc))
	switch rc {
	case C.KMB_ERR_CONNECTION_FAILED:
		return fmt.Errorf("%w: %s", ErrConnectionFailed, msg)
	case C.KMB_ERR_STREAM_NOT_FOUND:
		return fmt.Errorf("%w: %s", ErrStreamNotFound, msg)
	case C.KMB_ERR_PERMISSION_DENIED:
		return fmt.Errorf("%w: %s", ErrPermissionDenied, msg)
	case C.KMB_ERR_TIMEOUT:
		return fmt.Errorf("%w: %s", ErrTimeout, msg)
	case C.KMB_ERR_QUERY_SYNTAX, C.KMB_ERR_QUERY_EXECUTION:
		return fmt.Errorf("%w: %s", ErrQueryFailed, msg)
	default:
		return &KimberliteError{
			Code:    fmt.Sprintf("%d", int(rc)),
			Message: msg,
		}
	}
}

// convertQueryResult converts a C KmbQueryResult pointer to a Go QueryResult.
func convertQueryResult(r *C.KmbQueryResult) *QueryResult {
	colCount := int(r.column_count)
	rowCount := int(r.row_count)

	columns := make([]string, colCount)
	if colCount > 0 && r.columns != nil {
		cols := (*[1 << 20]*C.char)(unsafe.Pointer(r.columns))[:colCount:colCount]
		for i, p := range cols {
			columns[i] = C.GoString(p)
		}
	}

	rows := make([]map[string]Value, rowCount)
	if rowCount > 0 && r.rows != nil {
		rowPtrs := (*[1 << 20]*C.KmbQueryValue)(unsafe.Pointer(r.rows))[:rowCount:rowCount]
		rowLens := (*[1 << 20]C.size_t)(unsafe.Pointer(r.row_lengths))[:rowCount:rowCount]
		for i, rowPtr := range rowPtrs {
			rowLen := int(rowLens[i])
			row := make(map[string]Value, rowLen)
			if rowPtr != nil && rowLen > 0 {
				vals := (*[1 << 20]C.KmbQueryValue)(unsafe.Pointer(rowPtr))[:rowLen:rowLen]
				for j, v := range vals {
					colName := ""
					if j < colCount {
						colName = columns[j]
					}
					row[colName] = convertQueryValue(v)
				}
			}
			rows[i] = row
		}
	}

	return &QueryResult{Columns: columns, Rows: rows}
}

// convertQueryValue converts a C KmbQueryValue to a Go Value.
func convertQueryValue(v C.KmbQueryValue) Value {
	switch int(v.value_type) {
	case C.KMB_VALUE_BIGINT:
		return NewInt(int64(v.bigint_val))
	case C.KMB_VALUE_TEXT:
		if v.text_val != nil {
			return NewText(C.GoString(v.text_val))
		}
		return NewText("")
	case C.KMB_VALUE_BOOLEAN:
		return NewBool(v.bool_val != 0)
	case C.KMB_VALUE_TIMESTAMP:
		return NewTimestamp(time.Unix(0, int64(v.timestamp_val)))
	default:
		return NewNull()
	}
}
