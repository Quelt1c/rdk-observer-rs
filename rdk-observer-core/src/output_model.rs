//! Defines the observation records prepared for output encoding.
//!
//! Each observation identifies one process and carries sampling timestamps with
//! memory, CPU, storage I/O, or a collection of thread records. Thread entries
//! include their identities, metadata, and optional CPU and I/O measurements.
//! Process identities include optional logical and security labels.
//!
//! These types describe the data passed to `output_encoding`, which will map
//! records to shared Protobuf messages and serialize them using `prost`.

// -----------------------------------------------------------------------------
// Process record
// -----------------------------------------------------------------------------

/// A timestamped resource observation associated with a process.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservationRecord {
    /// Identity of the process to which this observation belongs.
    pub process_identity: ProcessIdentity,
    /// Sampling time in nanoseconds since the Unix epoch.
    pub timestamp_unix_ns: u64,
    /// Sampling time in nanoseconds on the monotonic clock.
    pub timestamp_monotonic_ns: u64,
    /// Resource measurements captured by this observation.
    pub resource: ResourceRecord,
}

/// Resource measurements for memory, CPU, storage I/O, or a collection of threads.
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceRecord {
    Memory(MemoryRecord),
    Cpu(CpuRecord),
    Storage(StorageRecord),
    Threads(Vec<ThreadRecord>),
}

// -----------------------------------------------------------------------------
// Process identification
// -----------------------------------------------------------------------------

/// Identifies a process by PID and start time, with optional descriptive labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    /// Operating system process ID.
    pub pid: u32,
    /// Process start time in clock ticks since system boot, used to detect PID reuse.
    pub start_time_ticks: u64,
    /// Optional logical name used to describe or group the process.
    pub logical_label: Option<String>,
    /// Optional security context assigned to the process.
    pub security_label: Option<SecurityLabel>,
}

/// A process security label paired with the security module that supplies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityLabel {
    /// Name of the security module that supplies the label.
    pub module: String,
    /// Security label text reported by the module.
    pub value: String,
}

// -----------------------------------------------------------------------------
// CPU record
// -----------------------------------------------------------------------------

/// CPU counters with their clock frequency and optional utilization measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct CpuRecord {
    /// Number of clock ticks per second, used to convert CPU counters to seconds.
    pub clock_ticks_per_second: u64,
    /// Cumulative user and system CPU time counters.
    pub counters: CpuCounters,
    /// Optional CPU usage percentage derived from consecutive samples.
    pub utilization: Option<CpuUtilization>,
}

/// Cumulative CPU time spent in user and system modes, measured in clock ticks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CpuCounters {
    /// Cumulative CPU time spent in user mode, in clock ticks, when available.
    pub user_ticks: Option<u64>,
    /// Cumulative CPU time spent in kernel mode, in clock ticks, when available.
    pub system_ticks: Option<u64>,
}

/// CPU utilization as a percentage over a sampling interval in nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuUtilization {
    /// Elapsed time between the samples used to calculate utilization, in nanoseconds.
    pub interval_ns: u64,
    /// CPU usage over the sampling interval, expressed as a percentage.
    pub percent: f64,
}

// -----------------------------------------------------------------------------
// Memory record
// -----------------------------------------------------------------------------

/// Optional virtual, resident, and proportional memory measurements in bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryRecord {
    /// Total virtual address space size in bytes, when available.
    pub virtual_bytes: Option<u64>,
    /// Memory currently resident in physical RAM in bytes, when available.
    pub resident_bytes: Option<u64>,
    /// Resident memory in bytes with shared pages divided among their users, when available.
    pub proportional_bytes: Option<u64>,
}

// -----------------------------------------------------------------------------
// I/O record
// -----------------------------------------------------------------------------

/// Cumulative storage I/O counters and optional transfer rates.
#[derive(Debug, Clone, PartialEq)]
pub struct StorageRecord {
    /// Cumulative storage I/O byte and syscall counts.
    pub counters: StorageCounters,
    /// Read and write rates derived from two valid consecutive samples.
    /// `None` means rates are unavailable, such as on the first sample, after a
    /// counter reset, or when the process identity changes; counters remain usable.
    /// A rate of zero means no I/O occurred during the interval.
    pub rates: Option<IoRates>,
}

/// Cumulative storage I/O byte counts and read and write syscall counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageCounters {
    /// Cumulative number of bytes read from storage, when available.
    pub read_bytes: Option<u64>,
    /// Cumulative number of bytes written to storage, when available.
    pub write_bytes: Option<u64>,
    /// Cumulative number of read I/O system calls, when available.
    pub read_syscalls: Option<u64>,
    /// Cumulative number of write I/O system calls, when available.
    pub write_syscalls: Option<u64>,
}

/// Read and write byte rates over a sampling interval in nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IoRates {
    /// Elapsed time between the samples used to calculate rates, in nanoseconds.
    pub interval_ns: u64,
    /// Average bytes read per second over the interval, when available.
    pub read_bytes_per_second: Option<f64>,
    /// Average bytes written per second over the interval, when available.
    pub write_bytes_per_second: Option<f64>,
}

// -----------------------------------------------------------------------------
// Thread record
// -----------------------------------------------------------------------------

/// Identifies a thread by its thread ID and start time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadIdentity {
    /// Operating system thread ID.
    pub tid: u32,
    /// Thread start time in clock ticks since system boot, used to detect TID reuse.
    pub start_time_ticks: u64,
}

/// A thread's identity, metadata, and optional CPU and storage measurements.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadRecord {
    /// Identity of the thread to which these measurements belong.
    pub identity: ThreadIdentity,
    /// Thread name reported by the operating system, when available.
    pub name: Option<String>,
    /// Operating system state code, such as 'R' for running or 'S' for sleeping.
    pub state: Option<char>,
    /// Raw scheduling priority reported by the operating system, when available.
    pub priority: Option<i64>,
    /// Nice value influencing scheduling priority; lower values favor the thread.
    pub nice: Option<i32>,
    /// Index of the CPU on which the thread last executed, when available.
    pub last_cpu: Option<u32>,
    /// Cumulative context switches caused by the thread yielding or blocking.
    pub voluntary_context_switches: Option<u64>,
    /// Cumulative context switches caused by the scheduler preempting the thread.
    pub nonvoluntary_context_switches: Option<u64>,
    /// Optional CPU time counters and utilization for this thread.
    pub cpu: Option<CpuRecord>,
    /// Optional storage I/O counters and transfer rates for this thread.
    pub storage: Option<StorageRecord>,
}
