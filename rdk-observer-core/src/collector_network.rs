//! Network activity collection.
//!
//! This module will provide functions called by `collector_worker` according to
//! the configured scope. System readings may include traffic counters per
//! interface. Per-process readings will require a source that can attribute
//! traffic to a registered target; interface totals will not be presented as
//! an individual process's traffic.
//!
//! Sources, fields, and supported scopes remain to be defined by an ADR.
//! Unsupported process measurements will be represented as unavailable.
//! Readings will retain timestamps, identities, raw counters, and units for
//! processing by `output_worker`.
