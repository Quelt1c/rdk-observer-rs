//! Defines the observation records prepared for output encoding.
//!
//! Each observation identifies one process and carries sampling timestamps with
//! memory, CPU, storage I/O, or a collection of thread records. Thread entries
//! include their identities, metadata, and optional CPU and I/O measurements.
//! Process identities include optional logical and security labels.
//!
//! These types describe the data passed to `output_encoding`, which will map
//! records to shared Protobuf messages and serialize them using `prost`.

#[derive(Debug, Clone, PartialEq)]
pub struct ObservationRecord {
    pub process_identity: ProcessIdentity,
    pub timestamp_unix_ns: u64,
    pub timestamp_monotonic_ns: u64,
    pub resource: ResourceRecord,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResourceRecord {
    Memory(MemoryRecord),
    Cpu(CpuRecord),
    Storage(StorageRecord),
    Threads(Vec<ThreadRecord>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub start_time_ticks: u64,
    pub logical_label: Option<String>,
    pub security_label: Option<SecurityLabel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityLabel {
    pub module: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadIdentity {
    pub tid: u32,
    pub start_time_ticks: u64,
}

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryRecord {
    pub virtual_bytes: Option<u64>,
    pub resident_bytes: Option<u64>,
    pub proportional_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpuRecord {
    pub clock_ticks_per_second: u64,
    pub counters: CpuCounters,
    pub utilization: Option<CpuUtilization>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CpuCounters {
    pub user_ticks: Option<u64>,
    pub system_ticks: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuUtilization {
    pub interval_ns: u64,
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StorageRecord {
    pub counters: StorageCounters,
    pub rates: Option<IoRates>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageCounters {
    pub read_bytes: Option<u64>,
    pub write_bytes: Option<u64>,
    pub read_syscalls: Option<u64>,
    pub write_syscalls: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IoRates {
    pub interval_ns: u64,
    pub read_bytes_per_second: Option<f64>,
    pub write_bytes_per_second: Option<f64>,
}
