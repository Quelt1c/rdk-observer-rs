//! Observation record preparation, encoding, and buffering worker.
//!
//! This module will run a worker started by `runtime` to receive measurements
//! from `collector_worker` tasks through channels. It will prepare records while
//! preserving timestamps, PID/start-time identities, raw and derived
//! measurements, units, and the distinction between unavailable values and zero.
//!
//! Collection tasks will calculate configured derived measurements before
//! forwarding observations. System, per-process, and full-list observations
//! will remain distinguishable and will not imply an atomic view.
//!
//! The worker will use `output_encoding` and `output_model` to encode observations
//! as Protobuf, then queue them through `output_storage` in the 500 KB in-memory
//! ring buffer. Process lifecycle events will stay within discovery and
//! collection coordination.
//!
//! Consumption or delivery of buffered records by an external collector or
//! local consumer will be defined separately, using the Protobuf format from
//! ADR-0004. The consuming component, retrieval interface, record removal,
//! buffer overflow policy, and input channel backpressure remain to be defined.
//! Preparation, encoding, and buffering failures will be reported to the runtime.
//! Pending record handling during shutdown also remains to be defined.
