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
    pub process_identity: ProcessIdentity,
    pub timestamp_unix_ns: u64,
    pub timestamp_monotonic_ns: u64,
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
    pub pid: u32,
    pub start_time_ticks: u64,
    pub logical_label: Option<String>,
    pub security_label: Option<SecurityLabel>,
}

/// A process security label paired with the security module that supplies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityLabel {
    pub module: String,
    pub value: String,
}

// -----------------------------------------------------------------------------
// CPU record
// -----------------------------------------------------------------------------

/// CPU counters with their clock frequency and optional utilization measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct CpuRecord {
    pub clock_ticks_per_second: u64,
    pub counters: CpuCounters,
    pub utilization: Option<CpuUtilization>,
}

/// Cumulative CPU time spent in user and system modes, measured in clock ticks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CpuCounters {
    pub user_ticks: Option<u64>,
    pub system_ticks: Option<u64>,
}

/// CPU utilization as a percentage over a sampling interval in nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuUtilization {
    pub interval_ns: u64,
    pub percent: f64,
}

// -----------------------------------------------------------------------------
// Memory record
// -----------------------------------------------------------------------------

/// Optional virtual, resident, and proportional memory measurements in bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryRecord {
    pub virtual_bytes: Option<u64>,
    pub resident_bytes: Option<u64>,
    pub proportional_bytes: Option<u64>,
}

// -----------------------------------------------------------------------------
// I/O record
// -----------------------------------------------------------------------------

/// Cumulative storage I/O counters and optional transfer rates.
#[derive(Debug, Clone, PartialEq)]
pub struct StorageRecord {
    pub counters: StorageCounters,
    pub rates: Option<IoRates>,
}

/// Cumulative storage I/O byte counts and read and write syscall counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageCounters {
    pub read_bytes: Option<u64>,
    pub write_bytes: Option<u64>,
    pub read_syscalls: Option<u64>,
    pub write_syscalls: Option<u64>,
}

/// Read and write byte rates over a sampling interval in nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IoRates {
    pub interval_ns: u64,
    pub read_bytes_per_second: Option<f64>,
    pub write_bytes_per_second: Option<f64>,
}

// -----------------------------------------------------------------------------
// Thread record
// -----------------------------------------------------------------------------

/// Identifies a thread by its thread ID and start time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadIdentity {
    pub tid: u32,
    pub start_time_ticks: u64,
}

/// A thread's identity, metadata, and optional CPU and storage measurements.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadRecord {
    pub identity: ThreadIdentity,
    pub name: Option<String>,
    pub state: Option<char>,
    pub priority: Option<i64>,
    pub nice: Option<i32>,
    pub last_cpu: Option<u32>,
    pub voluntary_context_switches: Option<u64>,
    pub nonvoluntary_context_switches: Option<u64>,
    pub cpu: Option<CpuRecord>,
    pub storage: Option<StorageRecord>,
}
