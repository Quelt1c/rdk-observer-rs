use anyhow::{Context, bail};
use aya::programs::perf_event::{PerfEventConfig, PerfEventGroup, PerfEventScope, SoftwareEvent};
use procfs::process::Process;
use rustix::time::{ClockId, clock_gettime};
use std::collections::HashMap;
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::time::Duration;

const MAX_TRACKED_THREADS: usize = 4096;

#[derive(Debug, Clone, Copy)]
struct ThreadFaultBucket {
    tid: u32,
    minor: u64,
    major: u64,
}

struct GroupTracker {
    pid: u32,
    groups: HashMap<u32, PerfEventGroup>,
    dropped_threads: u64,
}

impl GroupTracker {
    fn new(pid: u32, expected_starttime: u64) -> anyhow::Result<Self> {
        let stat = Process::new(pid as i32)?.stat()?;
        if stat.starttime != expected_starttime {
            bail!(
                "PID reuse detected: expected {}, got {}",
                expected_starttime,
                stat.starttime
            );
        }
        Ok(Self {
            pid,
            groups: HashMap::new(),
            dropped_threads: 0,
        })
    }

    fn sync_threads(&mut self) -> anyhow::Result<()> {
        let proc = Process::new(self.pid as i32)?;
        for task in proc.tasks()? {
            let tid = task?.tid as u32;
            if self.groups.contains_key(&tid) {
                continue;
            }
            if self.groups.len() >= MAX_TRACKED_THREADS {
                self.dropped_threads += 1;
                continue;
            }
            let group = PerfEventGroup::open(
                PerfEventConfig::Software(SoftwareEvent::PageFaultsMin),
                [PerfEventConfig::Software(SoftwareEvent::PageFaultsMaj)],
                PerfEventScope::OneProcess {
                    pid: tid,
                    cpu: None,
                },
            )
            .with_context(|| format!("PerfEventGroup::open failed for tid={tid}"))?;

            group.reset()?;
            group.enable()?;
            self.groups.insert(tid, group);
        }
        Ok(())
    }

    fn read_bucket(&mut self) -> anyhow::Result<Vec<ThreadFaultBucket>> {
        let mut out = Vec::with_capacity(self.groups.len());
        let mut dead_tids = Vec::new();

        for (&tid, group) in &mut self.groups {
            match group.read() {
                Ok(result) => {
                    let minor = result.values[0];
                    let major = result.values[1];
                    let _ = group.reset();
                    if minor > 0 || major > 0 {
                        out.push(ThreadFaultBucket { tid, minor, major });
                    }
                }
                Err(_) => dead_tids.push(tid),
            }
        }
        for tid in dead_tids {
            self.groups.remove(&tid);
        }
        Ok(out)
    }
}

impl Drop for GroupTracker {
    fn drop(&mut self) {
        for (_, group) in self.groups.drain() {
            let _ = group.disable();
        }
    }
}

fn current_tid() -> u32 {
    unsafe { libc::syscall(libc::SYS_gettid) as u32 }
}

fn thread_minflt() -> i64 {
    let mut ru = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    unsafe {
        libc::getrusage(libc::RUSAGE_THREAD, ru.as_mut_ptr());
        ru.assume_init().ru_minflt
    }
}

fn rss_mib(pid: i32) -> u64 {
    let ps = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 };
    Process::new(pid).unwrap().statm().unwrap().resident * ps / (1024 * 1024)
}

fn fd_count() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}

fn main() -> anyhow::Result<()> {
    let pid = std::process::id();
    let starttime = Process::new(pid as i32)?.stat()?.starttime;
    let fd_before = fd_count();

    let ready_barrier = Arc::new(Barrier::new(3));
    let start_barrier = Arc::new(Barrier::new(3));
    let stop = Arc::new((Mutex::new(false), Condvar::new()));
    let (idle_tid, active_tid, ru_delta) = (
        Arc::new(Mutex::new(0u32)),
        Arc::new(Mutex::new(0u32)),
        Arc::new(Mutex::new(0i64)),
    );

    // Потік 1: сплячий
    let (rb1, sb1, s1, t1) = (
        Arc::clone(&ready_barrier),
        Arc::clone(&start_barrier),
        Arc::clone(&stop),
        Arc::clone(&idle_tid),
    );
    let h_idle = std::thread::spawn(move || {
        *t1.lock().unwrap() = current_tid();
        rb1.wait();
        sb1.wait();
        let (lk, cv) = &*s1;
        let mut g = lk.lock().unwrap();
        while !*g {
            g = cv.wait(g).unwrap();
        }
    });

    let (rb2, sb2, t2, r2) = (
        Arc::clone(&ready_barrier),
        Arc::clone(&start_barrier),
        Arc::clone(&active_tid),
        Arc::clone(&ru_delta),
    );
    let h_active = std::thread::spawn(move || {
        *t2.lock().unwrap() = current_tid();
        rb2.wait();
        sb2.wait();

        let before = thread_minflt();
        let (pages, ps) = (8192usize, unsafe {
            libc::sysconf(libc::_SC_PAGESIZE) as usize
        });
        let len = pages * ps;
        unsafe {
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            );
            assert_ne!(ptr, libc::MAP_FAILED);
            let buf = std::slice::from_raw_parts_mut(ptr.cast::<u8>(), len);
            for i in 0..pages {
                std::ptr::write_volatile(&mut buf[i * ps], 0xAB);
            }
            *r2.lock().unwrap() = thread_minflt() - before;
            std::thread::sleep(Duration::from_millis(200));
            libc::munmap(ptr, len);
        }
    });

    ready_barrier.wait();
    let mut tracker = GroupTracker::new(pid, starttime)?;
    tracker.sync_threads()?;
    println!(
        "[ATTACHED] pid={pid} starttime={starttime} tracked_tids={} rss_before={} MiB",
        tracker.groups.len(),
        rss_mib(pid as i32)
    );

    start_barrier.wait();
    std::thread::sleep(Duration::from_millis(100));

    let ts = clock_gettime(ClockId::Monotonic);
    let rss_peak = rss_mib(pid as i32);
    let bucket = tracker.read_bucket()?;

    h_active.join().unwrap();
    *stop.0.lock().unwrap() = true;
    stop.1.notify_all();
    h_idle.join().unwrap();

    println!(
        "[BUCKET] monotonic_ns={} rss_peak={rss_peak} MiB dropped_threads={}",
        ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64,
        tracker.dropped_threads
    );
    for b in &bucket {
        println!("  tid={} minor={} major={}", b.tid, b.minor, b.major);
    }
    println!(
        "[VERIFY] active_tid={} idle_tid={} getrusage_minflt_delta={}",
        *active_tid.lock().unwrap(),
        *idle_tid.lock().unwrap(),
        *ru_delta.lock().unwrap()
    );

    drop(tracker);
    let fd_after = fd_count();
    println!("[CLEANUP] fd_before={fd_before} fd_after={fd_after}");
    assert_eq!(fd_before, fd_after, "FD leak detected!");
    Ok(())
}
