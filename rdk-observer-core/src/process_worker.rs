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

use crate::process_discovery::{NetlinkProcessEvent, start_process_discovery};
use futures::StreamExt;
use procfs::ProcError;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::pin::pin;
use std::time::Duration;
use tokio::select;
use tokio::time::MissedTickBehavior;
use tracing::{error, info, warn};

/// Interval at which whole process list are refreshed
const PID_REFETCH_INTERVAL: Duration = Duration::from_secs(30);

/// Interval at which whole process list are refreshed
/// when connector socket connection fails to any reason
const PID_REFETCH_INTERVAL_FALLBACK: Duration = PID_REFETCH_INTERVAL;

/// Typed Process ID
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ProcessId(i32);

impl Display for ProcessId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

/// Resolved process info
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ProcessInfo {
    pid: ProcessId,
    command: String,
    start_time: u64,
    /// exit_code when `Ok`, exit_signal when `Err`
    exit_result: Option<Result<u32, u32>>,
}

/// Process lifecycle event
#[derive(Debug)]
pub enum ProcessEvent {
    Started(ProcessInfo),
    Finished(ProcessInfo),
}

pub async fn start_process_worker() {
    let mut processes = HashMap::new();
    let mut events = Vec::new();

    let mut discovery_stream = pin!(start_process_discovery());
    let mut refresh_interval = tokio::time::interval(PID_REFETCH_INTERVAL_FALLBACK);
    refresh_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        select! {
            event = discovery_stream.next() => {
                let Some(event) = event else {
                    warn!("process discovery restarted, should never happen");
                    discovery_stream.set(start_process_discovery());
                    continue;
                };
                match event {
                    NetlinkProcessEvent::Connected => {
                        refresh_interval = tokio::time::interval(PID_REFETCH_INTERVAL);
                        refresh_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
                    }
                    NetlinkProcessEvent::Disconnected => {
                        refresh_interval = tokio::time::interval(PID_REFETCH_INTERVAL_FALLBACK);
                        refresh_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
                    }
                    NetlinkProcessEvent::FullUpdateRequired => {
                        refresh_interval.reset_immediately();
                    }
                    NetlinkProcessEvent::Fork(e) => {
                        // new process is only started when pid equals to thread group id
                        // otherwise this event signals pthread_create + CLONE_THREAD (or similar)

                        if e.child_pid == e.child_tgid {
                            let process_id = ProcessId(e.child_tgid);
                            update_process(process_id, &mut events, &mut processes).await;
                        }
                    }
                    NetlinkProcessEvent::Exec(e) => {
                        let process_id = ProcessId(e.process_tgid);
                        update_process(process_id, &mut events, &mut processes).await;
                    }
                    NetlinkProcessEvent::Comm(e) => {
                        // only main thread rename changes process name,
                        // other threads can be renamed via pthread_setname_np

                        let process_id = ProcessId(e.process_tgid);
                        if e.process_pid == e.process_tgid
                            && let Some(process) = processes.get_mut(&process_id)
                        {
                            process.command = e.command;
                        }
                    }
                    NetlinkProcessEvent::Exit(e) => {
                        // 1.
                        // pid equals to thread group id only means that main thread has stopped.
                        // other thread can still be alive, so we need to wait until all of them are stopped

                        // 2.
                        // exit event of a previous pid owner may arrive after a new process with the same pid
                        // appears, so it is accepted only when the exit happened after the tracked process started

                        let process_id = ProcessId(e.process_tgid);
                        if e.process_pid == e.process_tgid
                            && let Some(process) = processes.get_mut(&process_id)
                            && e.ts_boot_ticks >= process.start_time
                        {
                            process.exit_result = Some(e.exit_result);
                        }

                        update_process(process_id, &mut events, &mut processes).await;
                    }
                }
            }
            _ = refresh_interval.tick() => {
                let pids = match read_all_pids().await {
                    Ok(e) => e,
                    Err(e) => {
                        error!("read_all_pids failed: {e}");
                        continue;
                    }
                };
                for (_, process) in processes.extract_if(|pid, _| !pids.contains(pid)) {
                    events.push(ProcessEvent::Finished(process));
                }
                for pid in pids {
                    update_process(pid, &mut events, &mut processes).await;
                }
            }
        }

        for event in events.drain(..) {
            info!("{event:?}");
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
async fn update_process(
    pid: ProcessId,
    events: &mut Vec<ProcessEvent>,
    processes: &mut HashMap<ProcessId, ProcessInfo>,
) {
    let Ok(process) = read_process_info(pid).await else {
        if let Some(process) = processes.remove(&pid) {
            events.push(ProcessEvent::Finished(process));
        }
        return;
    };

    match processes.entry(pid) {
        Entry::Occupied(mut e) => {
            if e.get().start_time == process.start_time {
                e.get_mut().command = process.command;
            } else {
                let old = e.insert(process.clone());
                events.push(ProcessEvent::Finished(old));
                events.push(ProcessEvent::Started(process));
            }
        }
        Entry::Vacant(e) => {
            e.insert(process.clone());
            events.push(ProcessEvent::Started(process));
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
async fn read_all_pids() -> anyhow::Result<HashSet<ProcessId>> {
    let task = tokio::task::spawn_blocking(|| {
        let mut pids = HashSet::new();
        for process in procfs::process::all_processes()? {
            match process {
                Ok(e) => {
                    pids.insert(ProcessId(e.pid));
                }
                Err(ProcError::NotFound(_)) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(pids)
    });
    task.await?
}

////////////////////////////////////////////////////////////////////////////////
async fn read_process_info(pid: ProcessId) -> anyhow::Result<ProcessInfo> {
    let task = tokio::task::spawn_blocking(move || {
        let stat = procfs::process::Process::new(pid.0)?.stat()?;
        Ok(ProcessInfo {
            pid,
            command: stat.comm,
            start_time: stat.starttime,
            exit_result: None,
        })
    });
    task.await?
}
