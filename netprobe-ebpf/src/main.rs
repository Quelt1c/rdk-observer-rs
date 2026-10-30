#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::bpf_get_current_pid_tgid,
    macros::{kretprobe, map},
    maps::PerCpuHashMap,
    programs::RetProbeContext,
};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawCounters {
    pub tcp_rx_bytes: u64,
    pub tcp_tx_bytes: u64,
    pub udp_rx_bytes: u64,
    pub udp_tx_bytes: u64,
}

#[derive(Clone, Copy)]
enum Protocol {
    Tcp,
    Udp,
}

#[map]
static PID_COUNTERS: PerCpuHashMap<i32, RawCounters> = PerCpuHashMap::with_max_entries(16384, 0);

#[inline(always)]
fn account(ctx: &RetProbeContext, proto: Protocol, transmit: bool) -> u32 {
    let ret = ctx.ret::<i64>();
    if ret <= 0 {
        return 0;
    }
    let bytes = ret as u64;
    let pid = (bpf_get_current_pid_tgid() >> 32) as i32;

    if let Some(counters_ptr) = PID_COUNTERS.get_ptr_mut(&pid) {
        let counters = unsafe { &mut *counters_ptr };
        match (proto, transmit) {
            (Protocol::Tcp, true) => counters.tcp_tx_bytes += bytes,
            (Protocol::Tcp, false) => counters.tcp_rx_bytes += bytes,
            (Protocol::Udp, true) => counters.udp_tx_bytes += bytes,
            (Protocol::Udp, false) => counters.udp_rx_bytes += bytes,
        }
        return 0;
    }

    let mut initial = RawCounters {
        tcp_rx_bytes: 0,
        tcp_tx_bytes: 0,
        udp_rx_bytes: 0,
        udp_tx_bytes: 0,
    };

    match (proto, transmit) {
        (Protocol::Tcp, true) => initial.tcp_tx_bytes = bytes,
        (Protocol::Tcp, false) => initial.tcp_rx_bytes = bytes,
        (Protocol::Udp, true) => initial.udp_tx_bytes = bytes,
        (Protocol::Udp, false) => initial.udp_rx_bytes = bytes,
    }

    let _ = PID_COUNTERS.insert(&pid, &initial, 0);

    0
}

#[kretprobe]
pub fn tcp_sendmsg_ret(ctx: RetProbeContext) -> u32 {
    account(&ctx, Protocol::Tcp, true)
}

#[kretprobe]
pub fn udp_sendmsg_ret(ctx: RetProbeContext) -> u32 {
    account(&ctx, Protocol::Udp, true)
}

#[kretprobe]
pub fn tcp_recvmsg_ret(ctx: RetProbeContext) -> u32 {
    account(&ctx, Protocol::Tcp, false)
}

#[kretprobe]
pub fn udp_recvmsg_ret(ctx: RetProbeContext) -> u32 {
    account(&ctx, Protocol::Udp, false)
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
