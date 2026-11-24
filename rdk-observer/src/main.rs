use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};
use procfs::process::Process;
use rdk_observer_core::page_fault_probe::{
    PageFaultBucket, PageFaultProbe, read_process_starttime,
};
use std::time::{Duration, Instant};
use tokio::time::MissedTickBehavior;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// All processes are captured by default, the options below narrow that scope
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Include the process with this PID; repeat to add more
    #[arg(short, long, value_name = "PID")]
    pid: Vec<u32>,
    /// Include processes with this command name; repeat to add more
    #[arg(short, long, value_name = "COMM")]
    command: Vec<String>,
    /// Include RDK processes from the configured selection list
    #[arg(long)]
    rdk_processes: bool,
    /// Exclude kernel threads
    #[arg(long)]
    no_kernel_threads: bool,
    /// Exclude userspace processes
    #[arg(long)]
    no_userspace_processes: bool,
    /// Enable eBPF per-thread page fault capture for specified --pid targets
    #[arg(long)]
    page_faults: bool,
    /// Aggregation bucket interval in milliseconds
    #[arg(long, default_value_t = 1000, value_name = "MS")]
    pf_bucket_ms: u64,
    /// Maximum capture duration in seconds
    #[arg(long, default_value_t = 10, value_name = "SECS")]
    pf_duration_secs: u64,
}

#[tokio::main]
async fn main() {
    let args = parse_arguments();

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_level(true)
                .with_span_events(FmtSpan::CLOSE),
        )
        .init();

    if args.page_faults {
        let bucket_interval = Duration::from_millis(args.pf_bucket_ms);
        let max_duration = Duration::from_secs(args.pf_duration_secs);

        if let Err(err) = run_page_fault_capture(&args.pid, bucket_interval, max_duration).await {
            error!(error = %err, "Page fault capture failed");
            std::process::exit(1);
        }
        return;
    }

    rdk_observer_core::runtime::run().await;
}

fn parse_arguments() -> Args {
    let args = Args::parse();

    if args.no_kernel_threads
        && args.no_userspace_processes
        && args.pid.is_empty()
        && args.command.is_empty()
        && !args.rdk_processes
    {
        Args::command()
            .error(
                ErrorKind::ArgumentConflict,
                "'--no-kernel-threads' and '--no-userspace-processes' together require at least one '--pid', '--command' or `rdk-processes`",
            )
            .exit();
    }

    if args.page_faults && args.pid.is_empty() {
        Args::command()
            .error(
                ErrorKind::MissingRequiredArgument,
                "'--page-faults' requires at least one '--pid <PID>'",
            )
            .exit();
    }

    args
}

async fn run_page_fault_capture(
    target_pids: &[u32],
    bucket_interval: Duration,
    max_duration: Duration,
) -> anyhow::Result<()> {
    let mut probe = PageFaultProbe::attach_to_all_cpus()?;

    for &pid in target_pids {
        let starttime = read_process_starttime(pid)?;
        probe.start_tracking_pid(pid, starttime)?;
        let rss_bytes = read_process_rss_bytes(pid);
        info!(pid, starttime, rss_bytes, "Tracking process page faults");
    }

    let deadline = Instant::now() + max_duration;
    let mut ticker = tokio::time::interval(bucket_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await;

    let mut bucket_index: u64 = 0;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Ctrl+C received, stopping capture");
                break;
            }
            _ = ticker.tick() => {
                bucket_index += 1;
                let bucket = probe.collect_and_reset_bucket()?;
                report_bucket(bucket_index, target_pids, &bucket);

                if Instant::now() >= deadline {
                    info!("Max capture duration reached");
                    break;
                }
            }
        }
    }

    for &pid in target_pids {
        if let Err(err) = probe.stop_tracking_pid(pid) {
            warn!(pid, error = %err, "Failed to untrack PID");
        }
    }

    drop(probe);
    info!("All eBPF probes and perf events detached");
    Ok(())
}

fn report_bucket(bucket_index: u64, target_pids: &[u32], bucket: &PageFaultBucket) {
    for &pid in target_pids {
        let rss_bytes = read_process_rss_bytes(pid);
        info!(
            bucket = bucket_index,
            monotonic_ns = bucket.monotonic_ns,
            pid,
            rss_bytes,
            dropped_faults = bucket.dropped_faults,
            "RSS sample (separate from fault counts)"
        );
    }

    for thread in &bucket.threads {
        info!(
            bucket = bucket_index,
            monotonic_ns = bucket.monotonic_ns,
            pid = thread.pid,
            tid = thread.tid,
            minor_faults = thread.minor_faults,
            major_faults = thread.major_faults,
            "Thread page faults"
        );
    }
}

fn read_process_rss_bytes(pid: u32) -> u64 {
    let page_size = rustix::param::page_size() as u64;
    let Ok(process) = Process::new(pid as i32) else {
        return 0;
    };
    let Ok(statm) = process.statm() else {
        return 0;
    };
    statm.resident * page_size
}
