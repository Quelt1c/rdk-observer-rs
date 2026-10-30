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

use anyhow::{Context, anyhow};
use aya::{
    Ebpf,
    maps::{MapError, PerCpuHashMap},
    programs::KProbe,
};
use procfs::net::dev_status;
use procfs::process::Process;
use std::collections::HashMap;
use std::sync::Mutex;

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

#[derive(Debug, Default)]
pub struct NetworkCounters {
    pub tcp_rx_bytes: Option<u64>,
    pub tcp_tx_bytes: Option<u64>,
    pub udp_rx_bytes: Option<u64>,
    pub udp_tx_bytes: Option<u64>,
}

#[derive(Debug)]
pub struct NetworkRecord {
    pub counters: NetworkCounters,
}

const PROBES: [(&str, &str); 4] = [
    ("tcp_sendmsg_ret", "tcp_sendmsg"),
    ("udp_sendmsg_ret", "udp_sendmsg"),
    ("tcp_recvmsg_ret", "tcp_recvmsg"),
    ("udp_recvmsg_ret", "udp_recvmsg"),
];

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RawCounters {
    tcp_rx_bytes: u64,
    tcp_tx_bytes: u64,
    udp_rx_bytes: u64,
    udp_tx_bytes: u64,
}

unsafe impl aya::Pod for RawCounters {}

struct NetworkProbe {
    ebpf: Ebpf,
}

static PROBE: Mutex<Option<NetworkProbe>> = Mutex::new(None);

impl NetworkProbe {
    fn load() -> anyhow::Result<Self> {
        let mut ebpf = Ebpf::load(aya::include_bytes_aligned!("../ebpf/netprobe-ebpf"))
            .context("failed to load eBPF object")?;

        for (program_name, symbol) in PROBES {
            let program: &mut KProbe = ebpf
                .program_mut(program_name)
                .with_context(|| format!("program {program_name} not found"))?
                .try_into()?;
            program
                .load()
                .with_context(|| format!("failed to load {program_name}"))?;
            program
                .attach(symbol, 0)
                .with_context(|| format!("failed to attach kretprobe to {symbol}"))?;
        }

        Ok(Self { ebpf })
    }

    fn read(&self, pid: i32) -> anyhow::Result<RawCounters> {
        let map = self
            .ebpf
            .map("PID_COUNTERS")
            .context("PID_COUNTERS map not found")?;
        let counters: PerCpuHashMap<_, i32, RawCounters> = PerCpuHashMap::try_from(map)?;

        match counters.get(&pid, 0) {
            Ok(per_cpu) => Ok(per_cpu
                .iter()
                .fold(RawCounters::default(), |acc, c| RawCounters {
                    tcp_rx_bytes: acc.tcp_rx_bytes.wrapping_add(c.tcp_rx_bytes),
                    tcp_tx_bytes: acc.tcp_tx_bytes.wrapping_add(c.tcp_tx_bytes),
                    udp_rx_bytes: acc.udp_rx_bytes.wrapping_add(c.udp_rx_bytes),
                    udp_tx_bytes: acc.udp_tx_bytes.wrapping_add(c.udp_tx_bytes),
                })),
            Err(MapError::KeyNotFound) => Ok(RawCounters::default()),
            Err(error) => Err(error).context("failed to read PID_COUNTERS"),
        }
    }
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

/// Reads network byte counters for a process through kretprobes on socket
/// send and receive functions.
///
/// Checks `expected_starttime` to reject a reused PID. Counters are
/// cumulative bytes since the probes were first loaded.
pub fn read_process_network(pid: i32, expected_starttime: u64) -> anyhow::Result<NetworkRecord> {
    let process = Process::new(pid).with_context(|| format!("failed to open process {pid}"))?;

    let stat = process
        .stat()
        .with_context(|| format!("failed to read /proc/{pid}/stat"))?;

    if stat.starttime != expected_starttime {
        return Err(anyhow!(
            "starttime mismatch for PID {pid}: expected {expected_starttime}, found {}",
            stat.starttime
        ));
    }

    let mut guard = PROBE
        .lock()
        .map_err(|_| anyhow!("network probe lock poisoned"))?;

    if guard.is_none() {
        *guard = Some(NetworkProbe::load()?);
    }

    let raw = guard
        .as_ref()
        .context("network probe is not initialized")?
        .read(pid)?;

    Ok(NetworkRecord {
        counters: NetworkCounters {
            tcp_rx_bytes: Some(raw.tcp_rx_bytes),
            tcp_tx_bytes: Some(raw.tcp_tx_bytes),
            udp_rx_bytes: Some(raw.udp_rx_bytes),
            udp_tx_bytes: Some(raw.udp_tx_bytes),
        },
    })
}
