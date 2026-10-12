//! System and per-process I/O counter collection.
//!
//! This module will provide functions to read I/O counters for a selected
//! process and system devices. `collector_worker` will pass registered identities
//! for individual reads or call these functions across the selected list.
//! The exact per-PID fields and accounting rules remain to be defined.
//!
//! Following ADR-0001, system readings will retain raw per-device counters from
//! `/proc/diskstats` and identify devices by major/minor number and name. Device
//! selection must avoid double-counting disks and their partitions. System and
//! process I/O accounting will remain distinguishable in the observations.
//!
//! Readings will retain timestamps, identities, counters, units, and unavailable
//! values. `output_worker` will derive rates from consecutive valid samples when
//! configured; counter resets, missing targets, or identity changes will
//! invalidate affected deltas. Filesystem capacity and free space are outside
//! ADR-0001's scope.
