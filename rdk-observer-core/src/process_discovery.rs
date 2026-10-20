//! Process target discovery and reconciliation worker.
//!
//! This module will run a worker started by `runtime` to discover candidate
//! targets in `normal-observer-mode` through an initial procfs inventory snapshot,
//! full reconciliation scans every 30 seconds, and netlink process connector
//! fork, exec, and exit events between scans. Subscribe to available connector
//! events before the startup scan and consume them while scanning.
//!
//! Discovery results and events will be forwarded to `process_worker`, which
//! will apply selection rules, validate identities, update `process_registry`,
//! and coordinate collection. Discovery scans will identify process targets;
//! their resource measurements will be collected by `collector_*`.
//!
//! ADR-0003 defines `NETLINK_CONNECTOR` / `CN_PROC` as the normal lifecycle
//! source. Reconcile scans with concurrent events and distinguish task exits
//! from whole-process completion. Partial scans must not imply target exits.
//! Detected event loss will request an early, coalesced reconciliation. If the
//! connector is unavailable, retain 30-second scans, report reduced coverage,
//! and retry subscription with bounded backoff. Short-lived processes may be
//! missed when events are unavailable or lost.
//!
//! Discovery and reconciliation will continue during `forensic-observer-mode`.
//! eBPF will be reserved for anomaly-triggered process or thread analysis, with
//! no eBPF programs loaded or attached for normal discovery. The reconciliation
//! cadence is independent of resource sampling intervals.
