package kimberlite

// CGo bindings to libkimberlite_ffi.
//
// The FFI library is built from the kimberlite-ffi crate and must be
// available at link time. Set CGO_LDFLAGS to point to the library:
//
//	CGO_LDFLAGS="-L/path/to/target/release -lkimberlite_ffi" go build

/*
#cgo LDFLAGS: -lkimberlite_ffi
#include <stdint.h>
#include <stdlib.h>

// FFI function declarations from libkimberlite_ffi
extern int32_t kmb_connect(const char* addr, uint64_t tenant_id, const char* token);
extern int32_t kmb_disconnect(void);
extern int32_t kmb_query(const char* sql, char** result_json, uint64_t* result_len);
extern int32_t kmb_create_stream(const char* name, int32_t data_class, char** result_json, uint64_t* result_len);
extern int32_t kmb_append(uint64_t stream_id, const uint8_t* data, uint64_t data_len, uint64_t* offset_out);
extern int32_t kmb_read_events(uint64_t stream_id, uint64_t from_offset, uint64_t max_bytes, char** result_json, uint64_t* result_len);
extern void kmb_free_string(char* ptr);
*/
import "C"

import (
	"encoding/json"
	"fmt"
	"unsafe"
)

// ffiAvailable returns true if the CGo FFI library is linked.
func ffiAvailable() bool {
	return true
}

func ffiConnect(addr string, tenantID uint64, token string) error {
	cAddr := C.CString(addr)
	defer C.free(unsafe.Pointer(cAddr))

	cToken := C.CString(token)
	defer C.free(unsafe.Pointer(cToken))

	rc := C.kmb_connect(cAddr, C.uint64_t(tenantID), cToken)
	if rc != 0 {
		return fmt.Errorf("ffi connect returned error code %d", rc)
	}
	return nil
}

func ffiDisconnect() error {
	rc := C.kmb_disconnect()
	if rc != 0 {
		return fmt.Errorf("ffi disconnect returned error code %d", rc)
	}
	return nil
}

func ffiQuery(sql string) (*QueryResult, error) {
	cSQL := C.CString(sql)
	defer C.free(unsafe.Pointer(cSQL))

	var resultJSON *C.char
	var resultLen C.uint64_t

	rc := C.kmb_query(cSQL, &resultJSON, &resultLen)
	if rc != 0 {
		return nil, fmt.Errorf("%w: ffi error code %d", ErrQueryFailed, rc)
	}
	defer C.kmb_free_string(resultJSON)

	data := C.GoBytes(unsafe.Pointer(resultJSON), C.int(resultLen))
	var result QueryResult
	if err := json.Unmarshal(data, &result); err != nil {
		return nil, fmt.Errorf("failed to decode query result: %w", err)
	}

	return &result, nil
}

func ffiCreateStream(name string, class DataClass) (*StreamInfo, error) {
	cName := C.CString(name)
	defer C.free(unsafe.Pointer(cName))

	var resultJSON *C.char
	var resultLen C.uint64_t

	rc := C.kmb_create_stream(cName, C.int32_t(class), &resultJSON, &resultLen)
	if rc != 0 {
		return nil, fmt.Errorf("ffi create_stream returned error code %d", rc)
	}
	defer C.kmb_free_string(resultJSON)

	data := C.GoBytes(unsafe.Pointer(resultJSON), C.int(resultLen))
	var info StreamInfo
	if err := json.Unmarshal(data, &info); err != nil {
		return nil, fmt.Errorf("failed to decode stream info: %w", err)
	}

	return &info, nil
}

func ffiAppend(streamID uint64, events [][]byte) (Offset, error) {
	// Concatenate all events for the FFI call
	var total int
	for _, e := range events {
		total += len(e)
	}
	buf := make([]byte, 0, total)
	for _, e := range events {
		buf = append(buf, e...)
	}

	var offsetOut C.uint64_t
	var dataPtr *C.uint8_t
	if len(buf) > 0 {
		dataPtr = (*C.uint8_t)(unsafe.Pointer(&buf[0]))
	}

	rc := C.kmb_append(C.uint64_t(streamID), dataPtr, C.uint64_t(len(buf)), &offsetOut)
	if rc != 0 {
		return 0, fmt.Errorf("ffi append returned error code %d", rc)
	}

	return Offset(offsetOut), nil
}

func ffiReadEvents(streamID, fromOffset, maxBytes uint64) ([]Event, error) {
	var resultJSON *C.char
	var resultLen C.uint64_t

	rc := C.kmb_read_events(C.uint64_t(streamID), C.uint64_t(fromOffset), C.uint64_t(maxBytes), &resultJSON, &resultLen)
	if rc != 0 {
		return nil, fmt.Errorf("ffi read_events returned error code %d", rc)
	}
	defer C.kmb_free_string(resultJSON)

	data := C.GoBytes(unsafe.Pointer(resultJSON), C.int(resultLen))
	var events []Event
	if err := json.Unmarshal(data, &events); err != nil {
		return nil, fmt.Errorf("failed to decode events: %w", err)
	}

	return events, nil
}
