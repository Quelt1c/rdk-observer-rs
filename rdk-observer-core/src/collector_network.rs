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

use crate::output_model::{ProcessNetworkCounters, ProcessNetworkRecord};
use anyhow::{Context, anyhow};
use aya::{
    Btf, Ebpf,
    maps::{MapError, PerCpuHashMap},
    programs::{FExit, KProbe},
};
use procfs::process::Process;
use std::path::Path;
use std::sync::Mutex;

const KRETPROBES: [(&str, &str); 4] = [
    ("tcp_sendmsg_ret", "tcp_sendmsg"),
    ("udp_sendmsg_ret", "udp_sendmsg"),
    ("tcp_recvmsg_ret", "tcp_recvmsg"),
    ("udp_recvmsg_ret", "udp_recvmsg"),
];

const FEXIT_PROBES: [(&str, &str); 4] = [
    ("tcp_sendmsg_fexit", "tcp_sendmsg"),
    ("udp_sendmsg_fexit", "udp_sendmsg"),
    ("tcp_recvmsg_fexit", "tcp_recvmsg"),
    ("udp_recvmsg_fexit", "udp_recvmsg"),
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

        let mut attached_via_fexit = false;

        if Path::new("/sys/kernel/btf/vmlinux").exists() {
            match Self::try_attach_fexit(&mut ebpf) {
                Ok(()) => {
                    println!("[probe] Successfully attached using FExit (BTF Trampoline)");
                    attached_via_fexit = true;
                }
                Err(err) => {
                    eprintln!(
                        "[probe] FExit attach failed ({err:#}), falling back to KRetProbe..."
                    );
                }
            }
        } else {
            println!("[probe] Kernel BTF not available, falling back to KRetProbe");
        }

        if !attached_via_fexit {
            Self::attach_kretprobes(&mut ebpf).context("failed to attach fallback kretprobes")?;
            println!("[probe] Successfully attached using KRetProbe");
        }

        Ok(Self { ebpf })
    }

    fn try_attach_fexit(ebpf: &mut Ebpf) -> anyhow::Result<()> {
        let btf =
            Btf::from_sys_fs().context("failed to load kernel BTF from /sys/kernel/btf/vmlinux")?;

        for (prog_name, target_fn) in FEXIT_PROBES {
            let prog: &mut FExit = ebpf
                .program_mut(prog_name)
                .with_context(|| format!("program {prog_name} not found"))?
                .try_into()?;

            prog.load(target_fn, &btf)
                .with_context(|| format!("failed to load fexit {prog_name} for {target_fn}"))?;

            prog.attach()
                .with_context(|| format!("failed to attach fexit {prog_name}"))?;
        }
        Ok(())
    }

    fn attach_kretprobes(ebpf: &mut Ebpf) -> anyhow::Result<()> {
        for (prog_name, target_fn) in KRETPROBES {
            let prog: &mut KProbe = ebpf
                .program_mut(prog_name)
                .with_context(|| format!("program {prog_name} not found"))?
                .try_into()?;

            prog.load()?;
            prog.attach(target_fn, 0)?;
        }
        Ok(())
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

    fn remove_pid(&mut self, pid: i32) -> anyhow::Result<()> {
        let map = self
            .ebpf
            .map_mut("PID_COUNTERS")
            .context("PID_COUNTERS map not found")?;
        let mut counters: PerCpuHashMap<_, i32, RawCounters> = PerCpuHashMap::try_from(map)?;

        match counters.remove(&pid) {
            Ok(_) | Err(MapError::KeyNotFound) => Ok(()),
            Err(error) => Err(error).context("failed to remove PID from PID_COUNTERS"),
        }
    }
}

/// Reads network byte counters for a process through kretprobes/fexit on socket
/// send and receive functions.
///
/// Checks `expected_starttime` to reject a reused PID. Returns `Ok(None)` if the
/// process no longer exists, evicting its entry from the eBPF map. Counters are
/// cumulative bytes since the probes were first loaded.
pub fn read_process_network(
    pid: i32,
    expected_starttime: u64,
) -> anyhow::Result<Option<ProcessNetworkRecord>> {
    let process = match Process::new(pid) {
        Ok(proc) => proc,
        Err(procfs::ProcError::NotFound(_)) => {
            if let Ok(mut guard) = PROBE.lock() {
                if let Some(ref mut probe) = *guard {
                    let _ = probe.remove_pid(pid);
                }
            }
            return Ok(None);
        }
        Err(err) => {
            return Err(anyhow::Error::from(err).context(format!("failed to open process {pid}")));
        }
    };

    let stat = match process.stat() {
        Ok(stat) => stat,
        Err(procfs::ProcError::NotFound(_)) => {
            if let Ok(mut guard) = PROBE.lock() {
                if let Some(ref mut probe) = *guard {
                    let _ = probe.remove_pid(pid);
                }
            }
            return Ok(None);
        }
        Err(err) => {
            return Err(
                anyhow::Error::from(err).context(format!("failed to read /proc/{pid}/stat"))
            );
        }
    };

    if stat.starttime != expected_starttime {
        if let Ok(mut guard) = PROBE.lock() {
            if let Some(ref mut probe) = *guard {
                let _ = probe.remove_pid(pid);
            }
        }
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

    Ok(Some(ProcessNetworkRecord {
        counters: ProcessNetworkCounters {
            tcp_rx_bytes: Some(raw.tcp_rx_bytes),
            tcp_tx_bytes: Some(raw.tcp_tx_bytes),
            udp_rx_bytes: Some(raw.udp_rx_bytes),
            udp_tx_bytes: Some(raw.udp_tx_bytes),
        },
        rates: None,
    }))
}
