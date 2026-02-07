package kimberlite

import (
	"fmt"
	"sync"
	"time"
)

// Client is the main entry point for interacting with a Kimberlite database.
type Client struct {
	mu       sync.RWMutex
	addr     string
	tenant   TenantID
	token    string
	timeout  time.Duration
	closed   bool
	ffiAvail bool
}

// Option configures a Client.
type Option func(*Client)

// WithTenant sets the tenant ID for the client connection.
func WithTenant(id uint64) Option {
	return func(c *Client) {
		c.tenant = TenantID(id)
	}
}

// WithToken sets the authentication token (JWT or API key).
func WithToken(token string) Option {
	return func(c *Client) {
		c.token = token
	}
}

// WithTimeout sets the default timeout for operations.
func WithTimeout(d time.Duration) Option {
	return func(c *Client) {
		c.timeout = d
	}
}

// Connect creates a new client and establishes a connection to the server.
func Connect(addr string, opts ...Option) (*Client, error) {
	c := &Client{
		addr:     addr,
		timeout:  30 * time.Second,
		ffiAvail: ffiAvailable(),
	}
	for _, opt := range opts {
		opt(c)
	}

	if c.tenant == 0 {
		return nil, ErrTenantRequired
	}

	if !c.ffiAvail {
		return nil, ErrFFIUnavailable
	}

	if err := c.connect(); err != nil {
		return nil, fmt.Errorf("%w: %s", ErrConnectionFailed, err)
	}

	return c, nil
}

// Close releases all resources associated with the client.
func (c *Client) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.closed {
		return nil
	}
	c.closed = true
	return c.disconnect()
}

// Query executes a SQL query and returns the results.
func (c *Client) Query(sql string) (*QueryResult, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()

	if c.closed {
		return nil, ErrNotConnected
	}

	return c.execQuery(sql)
}

// CreateStream creates a new event stream with the given name and data class.
func (c *Client) CreateStream(name string, class DataClass) (*StreamInfo, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()

	if c.closed {
		return nil, ErrNotConnected
	}

	return c.createStream(name, class)
}

// Append writes one or more events to a stream.
func (c *Client) Append(streamID StreamID, events ...[]byte) (Offset, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()

	if c.closed {
		return 0, ErrNotConnected
	}

	return c.appendEvents(streamID, events)
}

// ReadEvents reads events from a stream starting at the given offset.
func (c *Client) ReadEvents(streamID StreamID, from Offset, maxBytes uint64) ([]Event, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()

	if c.closed {
		return nil, ErrNotConnected
	}

	return c.readEvents(streamID, from, maxBytes)
}

// --- Internal FFI bridge (implemented in ffi.go) ---

func (c *Client) connect() error {
	return ffiConnect(c.addr, uint64(c.tenant), c.token)
}

func (c *Client) disconnect() error {
	return ffiDisconnect()
}

func (c *Client) execQuery(sql string) (*QueryResult, error) {
	return ffiQuery(sql)
}

func (c *Client) createStream(name string, class DataClass) (*StreamInfo, error) {
	return ffiCreateStream(name, class)
}

func (c *Client) appendEvents(streamID StreamID, events [][]byte) (Offset, error) {
	return ffiAppend(uint64(streamID), events)
}

func (c *Client) readEvents(streamID StreamID, from Offset, maxBytes uint64) ([]Event, error) {
	return ffiReadEvents(uint64(streamID), uint64(from), maxBytes)
}
