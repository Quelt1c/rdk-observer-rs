//! Network activity collection.
//!
//! This module will provide functions called by `collector_worker` according to
//! the configured scope. System readings may include traffic counters per
//! interface. Per-process readings will require a source that can attribute
//! traffic to a registered target; interface totals will not be presented as
//! an individual process's traffic.
//!
//! Sources, fields, and supported scopes remain to be defined by an ADR.
//! Unsupported process measurements will be represented as unavailable.
//! Readings will retain timestamps, identities, raw counters, and units for
//! processing by `output_worker`.

use anyhow::Context;
use procfs::net::dev_status;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct InterfaceCounters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
}

/// Reads network traffic counters per interface from `/proc/net/dev`.
///
/// Loopback interface (`lo`) is excluded.
/// Counters are cumulative raw values;
pub fn read_system_network() -> anyhow::Result<HashMap<String, InterfaceCounters>> {
    let statuses = dev_status().context("Failed to read network stats from procfs")?;
    let mut network_stats = HashMap::new();

    for (interface_name, status) in statuses {
        if interface_name == "lo" {
            continue;
        }

        let counters = InterfaceCounters {
            rx_bytes: status.recv_bytes,
            tx_bytes: status.sent_bytes,
            rx_packets: status.recv_packets,
            tx_packets: status.sent_packets,
            rx_errors: status.recv_errs,
            tx_errors: status.sent_errs,
            rx_dropped: status.recv_drop,
            tx_dropped: status.sent_drop,
        };

        network_stats.insert(interface_name, counters);
    }

    Ok(network_stats)
}
