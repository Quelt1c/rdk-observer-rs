//! System and per-process memory collection.
//!
//! This module will provide functions to collect memory readings for a selected
//! process and system totals. `collector_worker` will pass registered identities
//! for individual reads or call these functions for each target in a full
//! process collection pass, according to configuration.
//!
//! Following ADR-0001, system readings will include `MemTotal` and `MemAvailable`
//! from `/proc/meminfo`. Process readings will use per-PID procfs measurements;
//! the exact fields and accounting rules remain to be defined.
//! Readings will preserve timestamps, identities, values, units, and unavailable
//! measurements for preparation and encoding by `output_worker`. System and
//! process readings will be correlated by timestamp and need not sum exactly.
