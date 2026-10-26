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
//! values. `collector_worker` will derive rates from consecutive valid samples
//! when configured; counter resets, missing targets, or identity changes will
//! invalidate affected deltas. Filesystem capacity and free space are outside
//! ADR-0001's scope.
use crate::output_model::{
    BlockDeviceIdentity, StorageCounters, StorageRecord, SystemStorageCounters, SystemStorageRecord,
};
use anyhow::Context;
use std::fs;

pub async fn read_system_io() -> anyhow::Result<Vec<SystemStorageRecord>> {
    let stats = tokio::task::spawn_blocking(|| {
        let disks = procfs::diskstats().context("Failed to read /proc/diskstats")?;

        let physical_stats: Vec<SystemStorageRecord> = disks
            .into_iter()
            .filter(|d| is_physical_device(&d.name))
            .map(|d| SystemStorageRecord {
                device_identity: BlockDeviceIdentity {
                    major: d.major as u32,
                    minor: d.minor as u32,
                    name: d.name.clone(),
                },
                counters: SystemStorageCounters {
                    read_operations: Some(d.reads as u64),
                    write_operations: Some(d.writes as u64),
                    read_sectors: Some(d.sectors_read as u64),
                    write_sectors: Some(d.sectors_written as u64),
                },
                rates: None,
            })
            .collect();

        Ok::<_, anyhow::Error>(physical_stats)
    })
    .await
    .context("Failed to join system IO reader task")??;

    Ok(stats)
}

pub async fn read_process_io(pid: i32) -> anyhow::Result<StorageRecord> {
    let io = tokio::task::spawn_blocking(move || {
        procfs::process::Process::new(pid).and_then(|p| p.io())
    })
    .await
    .context("Failed to join process IO reader task")?
    .with_context(|| format!("Failed to read IO stats for PID {pid}"))?;

    Ok(StorageRecord {
        counters: StorageCounters {
            read_bytes: Some(io.read_bytes),
            write_bytes: Some(io.write_bytes),
            read_syscalls: Some(io.syscr),
            write_syscalls: Some(io.syscw),
        },
        rates: None,
    })
}

fn is_physical_device(name: &str) -> bool {
    let path = format!("/sys/class/block/{}", name);
    if let Ok(target_path) = fs::read_link(&path) {
        !target_path.to_string_lossy().contains("virtual")
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn process_io_matches_own_pid_and_counters_are_monotonic() {
        let pid = std::process::id() as i32;
        let first = read_process_io(pid)
            .await
            .expect("first read should succeed");
        let second = read_process_io(pid)
            .await
            .expect("second read should succeed");
        assert!(first.counters.read_bytes.is_some());
        assert!(second.counters.read_bytes.is_some());
        assert!(first.counters.write_bytes.is_some());
        assert!(second.counters.write_bytes.is_some());
        assert!(second.counters.read_bytes >= first.counters.read_bytes);
        assert!(second.counters.write_bytes >= first.counters.write_bytes);
    }

    #[tokio::test]
    async fn process_io_fails_for_nonexistent_pid() {
        let pid = i32::MAX;
        let err = read_process_io(pid)
            .await
            .expect_err("nonexistent PID must return an error");
        assert!(
            err.to_string()
                .contains(&format!("Failed to read IO stats for PID {pid}")),
            "unexpected error message: {err}"
        );
    }
}
