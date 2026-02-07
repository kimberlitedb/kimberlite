// Package kimberlite provides a Go client for the Kimberlite database.
//
// Kimberlite is a compliance-first database for regulated industries
// (healthcare, finance, legal) built on immutable, append-only logs.
//
// Quick start:
//
//	client, err := kimberlite.Connect("127.0.0.1:5432", kimberlite.WithTenant(1))
//	if err != nil {
//	    log.Fatal(err)
//	}
//	defer client.Close()
//
//	result, err := client.Query("SELECT * FROM patients")
package kimberlite

// Version is the current SDK version.
const Version = "0.5.0"
