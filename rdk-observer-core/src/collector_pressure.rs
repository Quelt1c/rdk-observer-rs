//! Resource pressure collection.
//!
//! This module will provide functions for CPU, memory, and I/O pressure readings,
//! called by `collector_worker` according to configuration. Process targets will
//! come from `process_registry` where the source supports process attribution.
//! System pressure will remain distinguishable from process measurements.
//!
//! Sources, fields, and supported scopes remain to be defined by an ADR.
//! Unsupported process measurements will be represented as unavailable.
//! Readings will retain timestamps, identities, values, and units for processing
//! and encoding by `output_worker`.
