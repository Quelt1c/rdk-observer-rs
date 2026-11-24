use anyhow::{Context, anyhow, bail};
use aya::{
    Ebpf,
    maps::{Array, HashMap as BpfHashMap, Map, MapData, PerCpuArray},
    programs::perf_event::{
        PerfEvent, PerfEventConfig, PerfEventScope, SamplePolicy, SoftwareEvent,
    },
    util::online_cpus,
};
use procfs::process::Process;
use rustix::time::{ClockId, clock_gettime};
use std::time::Duration;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct RawThreadCounters {
    pid: u32,
    _padding: u32,
    minor_faults: u64,
    major_faults: u64,
}
unsafe impl aya::Pod for RawThreadCounters {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadPageFaults {
    pub pid: u32,
    pub tid: u32,
    pub minor_faults: u64,
    pub major_faults: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageFaultBucket {
    pub monotonic_ns: u64,
    pub threads: Vec<ThreadPageFaults>,
    pub dropped_faults: u64,
}

pub struct PageFaultProbe {
    _ebpf: Ebpf,
    bank_cfg: Array<MapData, u32>,
    tracked: BpfHashMap<MapData, u32, u8>,
    banks: [BpfHashMap<MapData, u32, RawThreadCounters>; 2],
    dropped: PerCpuArray<MapData, u64>,
    active: usize,
}

fn take_map(ebpf: &mut Ebpf, name: &str) -> anyhow::Result<Map> {
    ebpf.take_map(name)
        .with_context(|| format!("{name} map not found"))
}

fn attach(ebpf: &mut Ebpf, name: &str, ev: SoftwareEvent, cpus: &[u32]) -> anyhow::Result<()> {
    let prog: &mut PerfEvent = ebpf
        .program_mut(name)
        .with_context(|| format!("eBPF program {name} not found"))?
        .try_into()?;
    prog.load()
        .with_context(|| format!("failed to load {name}"))?;
    for &cpu in cpus {
        prog.attach(
            PerfEventConfig::Software(ev),
            PerfEventScope::AllProcessesOneCpu { cpu },
            SamplePolicy::Period(1),
            false,
        )
        .with_context(|| format!("failed to attach {name} on CPU {cpu}"))?;
    }
    Ok(())
}

impl PageFaultProbe {
    pub fn attach_to_all_cpus() -> anyhow::Result<Self> {
        let mut ebpf = Ebpf::load(aya::include_bytes_aligned!("../ebpf/pagefault-ebpf"))
            .context("failed to load pagefault-ebpf")?;

        let mut bank_cfg: Array<_, u32> = take_map(&mut ebpf, "ACTIVE_BANK_CONFIG")?.try_into()?;
        bank_cfg.set(0, &0u32, 0)?;

        let cpus = online_cpus().map_err(|(msg, err)| anyhow!("{msg}: {err}"))?;
        attach(
            &mut ebpf,
            "on_minor_page_fault",
            SoftwareEvent::PageFaultsMin,
            &cpus,
        )?;
        attach(
            &mut ebpf,
            "on_major_page_fault",
            SoftwareEvent::PageFaultsMaj,
            &cpus,
        )?;

        Ok(Self {
            bank_cfg,
            tracked: take_map(&mut ebpf, "TRACKED_PIDS")?.try_into()?,
            banks: [
                take_map(&mut ebpf, "BUCKET_BANK_0")?.try_into()?,
                take_map(&mut ebpf, "BUCKET_BANK_1")?.try_into()?,
            ],
            dropped: take_map(&mut ebpf, "DROPPED_FAULTS_PER_CPU")?.try_into()?,
            _ebpf: ebpf,
            active: 0,
        })
    }

    pub fn start_tracking_pid(&mut self, pid: u32, expected_starttime: u64) -> anyhow::Result<()> {
        let actual = read_process_starttime(pid)?;
        if actual != expected_starttime {
            bail!(
                "PID reuse detected for pid={pid}: expected starttime={expected_starttime}, got {actual}"
            );
        }
        self.tracked.insert(&pid, &1, 0)?;
        Ok(())
    }

    pub fn stop_tracking_pid(&mut self, pid: u32) -> anyhow::Result<()> {
        let _ = self.tracked.remove(&pid);
        Ok(())
    }

    pub fn collect_and_reset_bucket(&mut self) -> anyhow::Result<PageFaultBucket> {
        let drain = self.active;
        self.active ^= 1;
        self.bank_cfg.set(0, &(self.active as u32), 0)?;

        std::thread::sleep(Duration::from_millis(1));

        let bank = &mut self.banks[drain];
        let items: Vec<(u32, RawThreadCounters)> = bank.iter().collect::<Result<_, _>>()?;
        let threads = items
            .into_iter()
            .map(|(tid, c)| {
                let _ = bank.remove(&tid);
                ThreadPageFaults {
                    pid: c.pid,
                    tid,
                    minor_faults: c.minor_faults,
                    major_faults: c.major_faults,
                }
            })
            .collect();

        Ok(PageFaultBucket {
            monotonic_ns: monotonic_timestamp_ns(),
            threads,
            dropped_faults: self.dropped.get(&0, 0)?.iter().sum(),
        })
    }
}

pub fn read_process_starttime(pid: u32) -> anyhow::Result<u64> {
    Ok(Process::new(pid as i32)?.stat()?.starttime)
}

pub fn monotonic_timestamp_ns() -> u64 {
    let ts = clock_gettime(ClockId::Monotonic);
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}
