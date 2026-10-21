//! System and per-process memory collection.
//!
//! This module will provide functions to collect memory readings for a selected
//! process and system totals. `collector_worker` will pass registered identities
//! for individual reads or call these functions for each target in a full
//! process collection pass, according to configuration.
//!
//! Following ADR-0001, system readings will include `MemTotal` and `MemAvailable`
//! from `/proc/meminfo`. Process readings will use per-PID procfs measurements;
//! the exact fields and accounting rules remain to be defined.
//! Readings will preserve timestamps, identities, values, units, and unavailable
//! measurements for preparation and encoding by `output_worker`. System and
//! process readings will be correlated by timestamp and need not sum exactly.

use anyhow::{anyhow, Context, Result};
use procfs::process::{Process, Stat, Status};
use procfs::{Current, Meminfo};
use std::time::SystemTime;

#[derive(Debug)]
pub struct SystemMemorySample {
    pub timestamp: SystemTime,
    pub meminfo: Meminfo,
}

#[derive(Debug)]
pub struct ProcessMemorySample {
    pub timestamp: SystemTime,
    pub pid: u32,
    pub starttime: u64,
    pub status: Status,
}

pub fn read_system_memory() -> Result<SystemMemorySample> {
    let meminfo = Meminfo::current().context("failed to read /proc/meminfo")?;

    Ok(create_system_memory_sample(meminfo))
}

fn create_system_memory_sample(meminfo: Meminfo) -> SystemMemorySample {
    SystemMemorySample {
        timestamp: SystemTime::now(),
        meminfo,
    }
}

pub fn read_process_memory(pid: u32, expected_starttime: u64) -> Result<ProcessMemorySample> {
    let process_pid = i32::try_from(pid).context("PID is too large for procfs")?;

    let process =
        Process::new(process_pid).with_context(|| format!("failed to open process {pid}"))?;

    let stat = process
        .stat()
        .with_context(|| format!("failed to read /proc/{pid}/stat"))?;

    let status = process
        .status()
        .with_context(|| format!("failed to read /proc/{pid}/status"))?;

    create_process_memory_sample(stat, status, pid, expected_starttime)
}

fn create_process_memory_sample(
    stat: Stat,
    status: Status,
    pid: u32,
    expected_starttime: u64,
) -> Result<ProcessMemorySample> {
    let actual_pid = u32::try_from(stat.pid).context("procfs returned an invalid PID")?;

    if actual_pid != pid {
        return Err(anyhow!(
            "unexpected PID: expected {pid}, found {actual_pid}"
        ));
    }

    if status.pid != stat.pid {
        return Err(anyhow!(
            "PID mismatch between stat and status: stat {}, status {}",
            stat.pid,
            status.pid
        ));
    }

    if stat.starttime != expected_starttime {
        return Err(anyhow!(
            "starttime mismatch for PID {pid}: expected {expected_starttime}, found {}",
            stat.starttime
        ));
    }

    Ok(ProcessMemorySample {
        timestamp: SystemTime::now(),
        pid,
        starttime: stat.starttime,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use procfs::{FromBufRead, FromRead};
    use std::io::Cursor;

    fn valid_meminfo() -> &'static str {
        "MemTotal:       100 kB\n\
         MemFree:        200 kB\n\
         MemAvailable:   300 kB\n\
         Buffers:        400 kB\n\
         Cached:         500 kB\n\
         SwapCached:     600 kB\n\
         Active:         700 kB\n\
         Inactive:       800 kB\n\
         SwapTotal:      900 kB\n\
         SwapFree:      1000 kB\n\
         Dirty:         1100 kB\n\
         Writeback:     1200 kB\n\
         Mapped:        1300 kB\n\
         Slab:          1400 kB\n\
         Committed_AS:  1500 kB\n\
         VmallocTotal:  1600 kB\n\
         VmallocUsed:   1700 kB\n\
         VmallocChunk:  1800 kB\n"
    }

    fn parse_meminfo(proc_meminfo: &str) -> Meminfo {
        Meminfo::from_buf_read(Cursor::new(proc_meminfo.as_bytes()))
            .expect("failed to parse memory information")
    }

    fn valid_process_stat() -> &'static str {
        "1234 (my process) S 1 2 3 4 5 6 7 8 9 10 111 222 13 14 15 16 17 18 555 20 21 22 23 \
         24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49\n"
    }

    fn valid_process_status() -> &'static str {
        "Name:   my process\n\
         State:  S (sleeping)\n\
         Tgid:   1234\n\
         Pid:    1234\n\
         PPid:   1\n\
         TracerPid:      0\n\
         Uid:    1000 1000 1000 1000\n\
         Gid:    1000 1000 1000 1000\n\
         FDSize: 64\n\
         Groups: 1000\n\
         VmRSS:  111 kB\n\
         RssAnon:        222 kB\n\
         RssFile:        333 kB\n\
         RssShmem:       444 kB\n\
         VmSwap: 555 kB\n\
         Threads:        1\n\
         SigQ:   0/0\n\
         SigPnd: 0000000000000000\n\
         ShdPnd: 0000000000000000\n\
         SigBlk: 0000000000000000\n\
         SigIgn: 0000000000000000\n\
         SigCgt: 0000000000000000\n\
         CapInh: 0000000000000000\n\
         CapPrm: 0000000000000000\n\
         CapEff: 0000000000000000\n"
    }

    fn parse_process_stat(proc_stat: &str) -> Stat {
        Stat::from_read(Cursor::new(proc_stat.as_bytes())).expect("failed to parse process stat")
    }

    fn parse_process_status(proc_status: &str) -> Status {
        Status::from_buf_read(Cursor::new(proc_status.as_bytes()))
            .expect("failed to parse process status")
    }

    #[test]
    fn test_create_system_memory_sample() {
        let meminfo = parse_meminfo(valid_meminfo());
        let system_memory_sample = create_system_memory_sample(meminfo);

        assert_eq!(system_memory_sample.meminfo.mem_total, 100 * 1024);
        assert_eq!(system_memory_sample.meminfo.mem_available, Some(300 * 1024));
        assert_eq!(system_memory_sample.meminfo.swap_total, 900 * 1024);
    }

    #[test]
    fn test_create_process_memory_sample() {
        let stat = parse_process_stat(valid_process_stat());
        let status = parse_process_status(valid_process_status());

        let process_memory_sample = create_process_memory_sample(stat, status, 1234, 555)
            .expect("failed to create process memory sample");

        assert_eq!(process_memory_sample.pid, 1234);
        assert_eq!(process_memory_sample.starttime, 555);
        assert_eq!(process_memory_sample.status.pid, 1234);
        assert_eq!(process_memory_sample.status.vmrss, Some(111));
        assert_eq!(process_memory_sample.status.vmswap, Some(555));
    }

    #[test]
    fn test_create_process_memory_sample_wrong_pid() {
        let stat = parse_process_stat(valid_process_stat());
        let status = parse_process_status(valid_process_status());

        assert!(create_process_memory_sample(stat, status, 4321, 555).is_err());
    }

    #[test]
    fn test_create_process_memory_sample_wrong_starttime() {
        let stat = parse_process_stat(valid_process_stat());
        let status = parse_process_status(valid_process_status());

        assert!(create_process_memory_sample(stat, status, 1234, 999).is_err());
    }
}
