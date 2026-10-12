//! Observer configuration.
//!
//! This module will represent input parameters as configuration for `runtime`
//! and its workers. It will define process-selection rules, discovery options,
//! enabled resource collectors, and collection scope: individual registered
//! processes, a full pass over the selected list, system totals, or a combination.
//! Each collector's supported scopes will determine the available measurements.
//!
//! Scheduling options will include an independent system sampling frequency,
//! per-process interval bounds, change thresholds, bounded jitter, and a shared
//! procfs concurrency limit, following ADRs 0001-0003. Output options will select
//! derived measurements and delivery to an external or local consumer.
//! Thresholds and interval bounds will be chosen from measurements on target
//! devices. Parameter parsing, validation, defaults, and the configuration
//! format remain to be defined.
