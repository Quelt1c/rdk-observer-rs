//! System and per-process CPU counter collection.
//!
//! This module will provide functions to read aggregate CPU counters from
//! `/proc/stat` for system totals, following ADR-0001, and user and kernel CPU
//! time counters from `/proc/<pid>/stat` for a selected process.
//!
//! `collector_worker` will call these functions for individual registered
//! targets, a full pass over the selected list, or system totals, according to
//! configuration. Process reads will receive the PID and expected start time.
//!
//! Readings will retain timestamps, identities, raw counters, and units. Exited
//! or replaced targets will not produce a valid sample for the original process.
//! `collector_worker` will derive CPU utilization from consecutive valid samples
//! when configured. A first sample will establish a baseline; counter resets or
//! identity changes will invalidate the delta. System counters will provide
//! context when system and process observations are both enabled.
