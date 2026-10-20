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

use async_stream::stream;
use futures::Stream;
use libc::ENOBUFS;
use neli::connector::{CnMsg, CnMsgBuilder, ProcEvent, ProcEventHeader};
use neli::consts::connector::{CnMsgIdx, CnMsgVal, ProcCnMcastOp};
use neli::consts::nl::{NlmF, Nlmsg};
use neli::consts::socket::NlFamily;
use neli::err::SocketError;
use neli::nl::{NlPayload, NlmsghdrBuilder};
use neli::socket::asynchronous::NlSocketHandle;
use neli::utils::Groups;
use rustix::time::{ClockId, Timespec, clock_gettime};
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{debug, error, trace, warn};

////////////////////////////////////////////////////////////////////////////////
const RECONNECT_TIMEOUT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_TIMEOUT_MAX: Duration = Duration::from_mins(1);
const LISTEN_ACK_TIMEOUT: Duration = Duration::from_secs(5);

////////////////////////////////////////////////////////////////////////////////
#[derive(Debug)]
pub enum NetlinkProcessEvent {
    Connected,
    Disconnected,
    FullUpdateRequired,
    Fork(NetlinkProcessForkEvent),
    Exec(NetlinkProcessExecEvent),
    Comm(NetlinkProcessCommEvent),
    Exit(NetlinkProcessExitEvent),
}

#[derive(Debug)]
pub struct NetlinkProcessForkEvent {
    pub child_pid: i32,
    pub child_tgid: i32,
}

#[derive(Debug)]
pub struct NetlinkProcessExecEvent {
    pub process_pid: i32,
    pub process_tgid: i32,
}

#[derive(Debug)]
pub struct NetlinkProcessCommEvent {
    pub process_pid: i32,
    pub process_tgid: i32,
    pub command: String,
}

#[derive(Debug)]
pub struct NetlinkProcessExitEvent {
    pub process_pid: i32,
    pub process_tgid: i32,
    pub ts_boot_ticks: u64,
    pub exit_result: Result<u32, u32>,
}

////////////////////////////////////////////////////////////////////////////////
pub fn start_process_discovery() -> impl Stream<Item = NetlinkProcessEvent> {
    stream! {
        let mut timeout = RECONNECT_TIMEOUT_MIN;

        loop {
            debug!("opening connector socket");
            let Some(socket) = open_connector_socket() else {
                tokio::time::sleep(timeout).await;
                timeout = (timeout * 2).min(RECONNECT_TIMEOUT_MAX);
                continue;
            };

            debug!("registering as listener");
            if !register_listener(&socket).await {
                tokio::time::sleep(timeout).await;
                timeout = (timeout * 2).min(RECONNECT_TIMEOUT_MAX);
                continue;
            }

            debug!("socket registered");
            yield NetlinkProcessEvent::Connected;

            loop {
                let messages = match socket.recv::<Nlmsg, CnMsg<ProcEventHeader>>().await {
                    Ok((e, _)) => {
                        timeout = RECONNECT_TIMEOUT_MIN;
                        e
                    },
                    Err(SocketError::Io(e)) if e.raw_os_error() == Some(ENOBUFS) => {
                        warn!("events lost, requesting full refresh");
                        timeout = RECONNECT_TIMEOUT_MIN;
                        yield NetlinkProcessEvent::FullUpdateRequired;
                        continue;
                    }
                    Err(e) => {
                        break error!("failed to read from socket: {e}");
                    },
                };

                for message in messages {
                    let Some(message) = message.ok() else {
                        continue;
                    };
                    let Some(payload) = message.get_payload() else {
                        continue;
                    };

                    match payload.payload().event {
                        ProcEvent::Fork{ child_pid, child_tgid, .. } => {
                            trace!(child_pid, child_tgid, "netlink connector: fork");
                            yield NetlinkProcessEvent::Fork(NetlinkProcessForkEvent {
                                child_pid,
                                child_tgid,
                            });
                        }
                        ProcEvent::Exec{ process_pid, process_tgid } => {
                            trace!(process_pid, process_tgid, "netlink connector: exec");
                            yield NetlinkProcessEvent::Exec(NetlinkProcessExecEvent {
                                process_pid,
                                process_tgid,
                            });
                        }
                        ProcEvent::Comm{ process_pid, process_tgid, comm } => {
                            let command = comm.split(|e| *e == b'\0').next().unwrap_or(&comm);
                            let command = String::from_utf8_lossy(command).into_owned();

                            trace!(process_pid, process_tgid, command, "netlink connector: comm");
                            yield NetlinkProcessEvent::Comm(NetlinkProcessCommEvent {
                                process_pid,
                                process_tgid,
                                command,
                            });
                        }
                        ProcEvent::Exit{ process_pid, process_tgid, exit_code, .. } => {
                            let ts_boot_ticks = monotonic_ns_to_boot_ticks(payload.payload().timestamp_ns);
                            let exit_result = match exit_code & 0x7f {
                                0 => Ok((exit_code >> 8) & 0xff),
                                signal => Err(signal),
                            };

                            trace!(process_pid, process_tgid, ?exit_result, ts_boot_ticks, "netlink connector: exit");
                            yield NetlinkProcessEvent::Exit(NetlinkProcessExitEvent {
                                process_pid,
                                process_tgid,
                                exit_result,
                                ts_boot_ticks,
                            });
                        }
                        _ => {}
                    }
                }
            }

            yield NetlinkProcessEvent::Disconnected;
            tokio::time::sleep(timeout).await;
            timeout = (timeout * 2).min(RECONNECT_TIMEOUT_MAX);
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
fn open_connector_socket() -> Option<NlSocketHandle> {
    let groups = Groups::new_bitmask(CnMsgIdx::Proc.into());
    let socket = NlSocketHandle::connect(NlFamily::Connector, None, groups)
        .inspect_err(|e| error!("failed to open socket: {e}"))
        .ok()?;

    Some(socket)
}

////////////////////////////////////////////////////////////////////////////////
async fn register_listener(socket: &NlSocketHandle) -> bool {
    // kernel overwrites `seq` of the ack with its own event counter,
    // so only `ack` and `ack + 1` back can identify our request.
    let request_listen_ack = socket.pid();
    let response_listen_ack = request_listen_ack.wrapping_add(1);

    let Ok(listen_payload) = CnMsgBuilder::default()
        .idx(CnMsgIdx::Proc)
        .val(CnMsgVal::Proc)
        .ack(request_listen_ack)
        .payload(ProcCnMcastOp::Listen)
        .build()
        .inspect_err(|e| error!("failed to build listen payload: {e}"))
    else {
        return false;
    };

    let Ok(listen_message) = NlmsghdrBuilder::default()
        .nl_type(Nlmsg::Done)
        .nl_flags(NlmF::empty())
        .nl_pid(socket.pid())
        .nl_payload(NlPayload::Payload(listen_payload))
        .build()
        .inspect_err(|e| error!("failed to build listen message: {e}"))
    else {
        return false;
    };

    if let Err(e) = socket.send(&listen_message).await {
        error!("failed to send listen message: {e}");
        return false;
    }

    let deadline = tokio::time::Instant::now() + LISTEN_ACK_TIMEOUT;

    loop {
        let recv = socket.recv::<Nlmsg, CnMsg<ProcEventHeader>>();
        let messages = match tokio::time::timeout_at(deadline, recv).await {
            Ok(Ok((e, _))) => e,
            Ok(Err(SocketError::Io(e))) if e.raw_os_error() == Some(ENOBUFS) => continue,
            Ok(Err(e)) => {
                error!("failed to read listen ack: {e}");
                return false;
            }
            Err(_) => {
                error!("listen ack not received within {LISTEN_ACK_TIMEOUT:?}");
                return false;
            }
        };

        for message in messages {
            let Some(message) = message.ok() else {
                continue;
            };
            let Some(payload) = message.get_payload() else {
                continue;
            };
            if *payload.ack() != response_listen_ack {
                continue;
            }
            let ProcEvent::Ack { err } = payload.payload().event else {
                continue;
            };
            if err != 0 {
                let err = std::io::Error::from_raw_os_error(err as i32);
                error!("listen request rejected: {err}");
                return false;
            }
            return true;
        }
    }
}

/// Converts connector event timestamp_ns (`ClockId::Monotonic`, nanoseconds)
/// into `/proc/<pid>/stat` start_time (`ClockId::Boottime`, clock ticks)
fn monotonic_ns_to_boot_ticks(monotonic_ns: u64) -> u64 {
    static NS_TO_TICKS: OnceLock<u64> = OnceLock::new();

    fn timespec_to_ns(ts: Timespec) -> u64 {
        let s = ts.tv_sec as u64;
        let ns = ts.tv_nsec as u64;
        s.saturating_mul(1_000_000_000).saturating_add(ns)
    }

    let ns_to_ticks = *NS_TO_TICKS.get_or_init(|| 1_000_000_000 / procfs::ticks_per_second());
    let base_boot_time = clock_gettime(ClockId::Boottime);
    let base_monotonic = clock_gettime(ClockId::Monotonic);

    let time = monotonic_ns
        .saturating_add(timespec_to_ns(base_boot_time))
        .saturating_sub(timespec_to_ns(base_monotonic));

    time / ns_to_ticks
}
