//! Process target coordination worker.
//!
//! This module will run a coordinator started by `runtime` to receive discovered
//! processes and lifecycle events from `process_discovery`. It will apply the
//! configured selection rules and maintain targets in `process_registry`, using
//! PID and start time to identify each process.
//!
//! The coordinator will start and stop per-process `collector_worker` tasks as
//! targets enter or leave the registry. It will coalesce prompt collection
//! requests on fork or exec and request collection across the registered list
//! when a full process snapshot is selected. Collection modes will share the
//! configured concurrency limit and avoid duplicate work for a target.
//!
//! Lifecycle records will be sent to `output_worker` once it is ready, while
//! discovery continues tracking targets during startup. Resource measurements
//! will be collected by `collector_*` and sent directly to the same output
//! worker. Coordination failures will be reported to the runtime; the handling
//! policy remains to be defined.
