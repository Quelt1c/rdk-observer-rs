//! Process target discovery and reconciliation worker.
//!
//! This module will run a worker started by `runtime` to discover candidate
//! targets through full procfs scans or lifecycle events. Scans will find
//! already-running processes; events will identify processes that fork, execute,
//! or exit. Available event listeners will be attached before the startup scan.
//!
//! Discovery results and events will be forwarded to `process_worker`, which
//! will apply selection rules, validate identities, update `process_registry`,
//! and coordinate collection. Discovery scans will identify process targets;
//! their resource measurements will be collected by `collector_*`.
//!
//! ADR-0003 defines BPF events received through Aya's ring buffer. Netlink may
//! provide an alternative event source, but its integration remains to be
//! defined. Periodic procfs reconciliation will recover from missed events.
//! Procfs discovery will remain available when event capture cannot be used.
