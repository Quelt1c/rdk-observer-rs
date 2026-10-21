//! System and per-process CPU counter collection.
//!
//! This module will provide functions to read aggregate CPU counters from
//! `/proc/stat` for system totals, following ADR-0001, and user and kernel CPU
//! time counters from `/proc/<pid>/stat` for a selected process.
//!
//! `collector_worker` will call these functions for individual registered
//! targets, a full pass over the selected list, or system totals, according to
//! configuration. Process reads will receive the PID and expected start time.
//!
//! Readings will retain timestamps, identities, raw counters, and units. Exited
//! or replaced targets will not produce a valid sample for the original process.
//! `collector_worker` will derive CPU utilization from consecutive valid samples
//! when configured. A first sample will establish a baseline; counter resets or
//! identity changes will invalidate the delta. System counters will provide
//! context when system and process observations are both enabled.

use anyhow::{anyhow, Context, Result};
use procfs::process::{Process, Stat};
use procfs::{CpuTime, CurrentSI, KernelStats};
use std::time::SystemTime;

#[derive(Debug)]
pub struct SystemCpuSample {
    pub timestamp: SystemTime,
    pub counters: CpuTime,
}

#[derive(Debug)]
pub struct ProcessCpuSample {
    pub timestamp: SystemTime,
    pub stat: Stat,
}

pub fn read_system_cpu() -> Result<SystemCpuSample> {
    let kernel_stats = KernelStats::current().context("failed to read /proc/stat")?;

    Ok(create_system_cpu_sample(kernel_stats))
}

fn create_system_cpu_sample(kernel_stats: KernelStats) -> SystemCpuSample {
    SystemCpuSample {
        timestamp: SystemTime::now(),
        counters: kernel_stats.total,
    }
}

pub fn read_process_cpu(pid: u32, expected_starttime: u64) -> Result<ProcessCpuSample> {
    let process_pid = i32::try_from(pid).context("PID is too large for procfs")?;

    let process =
        Process::new(process_pid).with_context(|| format!("failed to open process {pid}"))?;

    let stat = process
        .stat()
        .with_context(|| format!("failed to read /proc/{pid}/stat"))?;

    create_process_cpu_sample(stat, pid, expected_starttime)
}

fn create_process_cpu_sample(
    stat: Stat,
    pid: u32,
    expected_starttime: u64,
) -> Result<ProcessCpuSample> {
    let actual_pid = u32::try_from(stat.pid).context("procfs returned an invalid PID")?;

    if actual_pid != pid {
        return Err(anyhow!(
            "unexpected PID: expected {pid}, found {actual_pid}"
        ));
    }

    if stat.starttime != expected_starttime {
        return Err(anyhow!(
            "starttime mismatch for PID {pid}: expected {expected_starttime}, found {}",
            stat.starttime
        ));
    }

    Ok(ProcessCpuSample {
        timestamp: SystemTime::now(),
        stat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use procfs::{FromBufReadSI, FromRead};
    use std::io::Cursor;

    fn valid_system_stat() -> &'static str {
        "cpu 100 200 300 400 500 600 700 800 900 1000\n\
         cpu0 10 20 30 40 50 60 70 80 90 100\n\
         intr 0\n\
         ctxt 1\n\
         btime 2\n\
         processes 3\n\
         procs_running 4\n\
         procs_blocked 5\n\
         softirq 0\n"
    }

    fn valid_process_stat() -> &'static str {
        "1234 (my process) S 1 2 3 4 5 6 7 8 9 10 111 222 13 14 15 16 17 18 555 20 21 22 23 \
         24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49\n"
    }

    fn parse_kernel_stats(proc_stat: &str) -> KernelStats {
        KernelStats::from_buf_read(
            Cursor::new(proc_stat.as_bytes()),
            procfs::current_system_info(),
        )
        .expect("failed to parse kernel stats")
    }

    fn parse_process_stat(proc_stat: &str) -> Stat {
        Stat::from_read(Cursor::new(proc_stat.as_bytes())).expect("failed to parse process stat")
    }

    #[test]
    fn test_create_system_cpu_sample() {
        let kernel_stats = parse_kernel_stats(valid_system_stat());
        let system_cpu_sample = create_system_cpu_sample(kernel_stats);

        assert_eq!(system_cpu_sample.counters.user, 100);
        assert_eq!(system_cpu_sample.counters.nice, 200);
        assert_eq!(system_cpu_sample.counters.system, 300);
        assert_eq!(system_cpu_sample.counters.idle, 400);
        assert_eq!(system_cpu_sample.counters.iowait, Some(500));
        assert_eq!(system_cpu_sample.counters.irq, Some(600));
        assert_eq!(system_cpu_sample.counters.softirq, Some(700));
        assert_eq!(system_cpu_sample.counters.steal, Some(800));
        assert_eq!(system_cpu_sample.counters.guest, Some(900));
        assert_eq!(system_cpu_sample.counters.guest_nice, Some(1000));
    }

    #[test]
    fn test_create_process_cpu_sample() {
        let stat = parse_process_stat(valid_process_stat());
        let process_cpu_sample =
            create_process_cpu_sample(stat, 1234, 555).expect("failed to create process sample");

        assert_eq!(process_cpu_sample.stat.pid, 1234);
        assert_eq!(process_cpu_sample.stat.starttime, 555);
        assert_eq!(process_cpu_sample.stat.utime, 111);
        assert_eq!(process_cpu_sample.stat.stime, 222);
    }

    #[test]
    fn test_create_process_cpu_sample_wrong_pid() {
        let stat = parse_process_stat(valid_process_stat());

        assert!(create_process_cpu_sample(stat, 4321, 555).is_err());
    }

    #[test]
    fn test_create_process_cpu_sample_wrong_starttime() {
        let stat = parse_process_stat(valid_process_stat());

        assert!(create_process_cpu_sample(stat, 1234, 999).is_err());
    }
}
