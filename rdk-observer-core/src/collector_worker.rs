//! Collection worker for process targets and system totals.
//!
//! This module will run collection tasks for the configured resources and scope.
//! Process collection will use identities from `process_registry` to read one
//! selected PID or collect a full process snapshot by sampling the registered
//! list. System collection will read device-wide totals independently of process
//! selection, following ADR-0001.
//!
//! `process_worker` will coordinate collection tasks for registered targets;
//! `runtime` will manage system collection and the overall worker lifecycle.
//! Tasks will call the applicable `collector_*` functions and combine readings
//! into timestamped observations, preserving raw values, units, process
//! identities, and unavailable measurements.
//!
//! Per-process tasks will use adaptive intervals and bounded jitter, following
//! ADR-0002. Periodic and event-triggered reads will share a concurrency limit.
//! Full-list requests will be coordinated with active tasks to avoid duplicate
//! reads. Delayed work will not cause bursts of catch-up samples.
//!
//! Tasks will wait for output readiness before producing observations, following
//! the startup order managed by `runtime`. Observations will be sent directly
//! to `output_worker` through a channel.
//! Full-list collection will use bounded concurrency and will not produce an
//! atomic snapshot of all processes or the whole system. Collection and channel
//! failures will be reported to the coordinator or runtime; policies remain
//! to be defined.
