//! Observation preparation, encoding, buffering, and delivery worker.
//!
//! This module will run a worker started by `runtime` to receive measurements
//! from `collector_worker` tasks and lifecycle records from `process_worker`
//! through channels. It will prepare records while preserving timestamps,
//! PID/start-time identities, raw measurements, units, and the distinction
//! between unavailable values and zero.
//!
//! When configured, preparation will derive CPU utilization and I/O rates from
//! consecutive valid samples with elapsed time between them. A first sample will
//! establish a baseline; counter resets, missing targets, or identity changes
//! will invalidate affected deltas. System, per-process, and full-list
//! observations will remain distinguishable and will not imply an atomic view.
//!
//! The worker will use `output_encoding` and `output_model` to encode observations
//! as Protobuf, then queue them through `output_storage` in the 500 KB in-memory
//! ring buffer. It will retrieve records and route them to the configured
//! external collector or local processing consumer. Local consumers will use
//! the same Protobuf schema; their interface and behavior remain to be defined.
//!
//! Following ADR-0004, the delivery protocol will define binary message
//! boundaries and batching. Transport, delivery schedules, retry policies,
//! record removal, local delivery behavior, and input channel backpressure
//! remain to be defined. Preparation, encoding, buffering, and delivery failures
//! will be reported to the runtime. Pending record handling during shutdown
//! also remains to be defined.
