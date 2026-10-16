//! Process target coordination worker.
//!
//! This module will run a coordinator started by `runtime` to receive discovered
//! processes and lifecycle events from `process_discovery`. It will apply the
//! configured selection rules and maintain targets in `process_registry`, using
//! PID and start time to identify each process.
//!
//! Scan results and netlink events will be reconciled without duplicating
//! targets or undoing newer lifecycle changes. Task IDs and thread-group IDs
//! must be distinguished: a thread exit is not proof of whole-process exit.
//! Validate ambiguous exits and delayed events before removing an identity,
//! preserving a live process that reused a PID. Partial scans must not evict
//! targets merely because identity information could not be read.
//!
//! The coordinator will start and stop per-process `collector_worker` tasks as
//! targets enter or leave the registry. It will coalesce prompt collection
//! requests on fork or exec and request collection across the registered list
//! when a full process snapshot is selected. Collection modes will share the
//! configured concurrency limit and avoid duplicate work for a target.
//!
//! Normal coordination will continue during forensic sessions. Anomaly triggers
//! routed to `runtime` will identify the affected process or thread for targeted
//! eBPF capture; routine lifecycle events and adaptive sampling changes alone
//! will not activate forensic mode. Target exit will also end its forensic
//! session, following ADR-0003.
//!
//! Collection tasks will report completion, target disappearance, and failures
//! to this coordinator so it can reconcile collection state with the registry.
//! Process lifecycle events will stay within discovery and collection
//! coordination. Resource observations will be collected by `collector_*` and
//! sent directly to `output_worker` for Protobuf preparation and storage.
//! Discovery and coordination will continue while output initializes.
//! Coordination failures will be reported to the runtime; the handling policy
//! remains to be defined.
