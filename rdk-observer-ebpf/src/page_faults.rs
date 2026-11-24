#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::bpf_get_current_pid_tgid,
    macros::{map, perf_event},
    maps::{Array, HashMap, PerCpuArray},
    programs::PerfEventContext,
};

const MAX_TRACKED_PIDS: u32 = 64;
const MAX_TRACKED_THREADS_PER_BUCKET: u32 = 4096;
const ACTIVE_BANK_INDEX: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ThreadFaultCounters {
    pub pid: u32,
    pub _padding: u32,
    pub minor_faults: u64,
    pub major_faults: u64,
}

#[map]
static ACTIVE_BANK_CONFIG: Array<u32> = Array::with_max_entries(1, 0);

#[map]
static TRACKED_PIDS: HashMap<u32, u8> = HashMap::with_max_entries(MAX_TRACKED_PIDS, 0);

#[map]
static BUCKET_BANK_0: HashMap<u32, ThreadFaultCounters> =
    HashMap::with_max_entries(MAX_TRACKED_THREADS_PER_BUCKET, 0);

#[map]
static BUCKET_BANK_1: HashMap<u32, ThreadFaultCounters> =
    HashMap::with_max_entries(MAX_TRACKED_THREADS_PER_BUCKET, 0);

#[map]
static DROPPED_FAULTS_PER_CPU: PerCpuArray<u64> = PerCpuArray::with_max_entries(1, 0);

#[perf_event]
pub fn on_minor_page_fault(_ctx: PerfEventContext) -> u32 {
    record_page_fault(true);
    0
}

#[perf_event]
pub fn on_major_page_fault(_ctx: PerfEventContext) -> u32 {
    record_page_fault(false);
    0
}

#[inline(always)]
fn increment_counter(counters: *mut ThreadFaultCounters, is_minor: bool) {
    unsafe {
        if is_minor {
            (*counters).minor_faults = (*counters).minor_faults.saturating_add(1);
        } else {
            (*counters).major_faults = (*counters).major_faults.saturating_add(1);
        }
    }
}

#[inline(always)]
fn increment_dropped_counter() {
    if let Some(dropped_ptr) = DROPPED_FAULTS_PER_CPU.get_ptr_mut(0) {
        unsafe {
            *dropped_ptr = (*dropped_ptr).saturating_add(1);
        }
    }
}

#[inline(always)]
fn record_page_fault(is_minor: bool) {
    let pid_tgid = bpf_get_current_pid_tgid();
    let pid = (pid_tgid >> 32) as u32;

    let is_pid_tracked = unsafe { TRACKED_PIDS.get(&pid) }.is_some();
    if !is_pid_tracked {
        return;
    }

    let tid = pid_tgid as u32;
    let active_bank = ACTIVE_BANK_CONFIG
        .get(ACTIVE_BANK_INDEX)
        .copied()
        .unwrap_or(0);

    let active_map = if active_bank == 0 {
        &BUCKET_BANK_0
    } else {
        &BUCKET_BANK_1
    };

    if let Some(existing_counters) = active_map.get_ptr_mut(&tid) {
        increment_counter(existing_counters, is_minor);
        return;
    }

    let initial_counters = ThreadFaultCounters {
        pid,
        _padding: 0,
        minor_faults: u64::from(is_minor),
        major_faults: u64::from(!is_minor),
    };

    if active_map.insert(&tid, &initial_counters, 1).is_err() {
        if let Some(existing_counters) = active_map.get_ptr_mut(&tid) {
            increment_counter(existing_counters, is_minor);
        } else {
            increment_dropped_counter();
        }
    }
}

#[panic_handler]
fn on_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
