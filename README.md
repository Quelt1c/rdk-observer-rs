# rdk-observer-rs

`rdk-observer-rs` is a system process observer for RDK devices. Its goal is to discover processes, collect resource measurements, and prepare Protobuf records for an external collector or local processing consumer.

This repository is currently a project scaffold. The executable entry point exists; the source modules document the intended design, while the workers and delivery mechanisms are not implemented yet.

## Running the scaffold

```sh
cargo run --bin rdk-observer
```

[`main.rs`](rdk-observer-core/src/main.rs) calls the library's `runtime::run()` function. It returns exit code 0 on success, or prints the runtime error to standard error and returns exit code 1 on failure. The runtime currently returns an explicit error stating that the observer workers are not implemented, so running the scaffold exits with code 1 and starts no monitoring.

Argument parsing, configuration loading, and logging initialization will be added to `main.rs` when their interfaces are defined. Worker orchestration, observer mode transitions, and shutdown belong in `runtime.rs`.

## Planned tracking options

The observer will support the following process selection options:

- All user-space processes.
- Kernel processes, where the system exposes them through `/proc`.
- User-space processes associated with RDK-B.
- Individually selected processes.

These options will allow deployments to choose between broad process monitoring and a selected subset. Normal observation will track processes; forensic observation may target individual threads within an anomalous process. System-wide resource totals will provide overall device context.

## Program design

The program separates target management, measurement collection, and output handling:

| Modules | Responsibility |
| --- | --- |
| `process_*` | Discover candidate processes, select targets, and maintain the process registry. |
| `collector_*` | Collect measurements for registered targets or system totals. |
| `output_*` | Prepare observations, encode Protobuf, buffer records, and route them to a consumer. |
| `config` | Represent input options, including target selection, resources, sampling, forensic capture policy, and output destination. |
| `runtime` | Start and connect the required Tokio workers, manage their lifecycle, and coordinate observer mode transitions. |

The entry point will eventually read input parameters, build and validate configuration, and launch the configured `runtime`. Configuration will determine which processes and resources to monitor, the collection scope, and whether records go to an external or local consumer.

```mermaid
flowchart TD
    Config["Input parameters and configuration"] --> Runtime["runtime"]
    Runtime -->|1. Start process workers| Processes["process_*: discovery and registry"]
    Runtime -->|2. Start collection workers| Collectors["collector_*: measurement collection"]
    Runtime -->|3. Initialize storage and start output worker| Output["output_*: preparation, encoding, and buffering"]
    Processes -->|Registered targets| Collectors
    Processes -->|Lifecycle records| Output
    Collectors -->|Timestamped observations| Output
    Output -->|Protobuf records| Consumer["External collector or local consumer"]
```

### Observer modes

The observer will start in `normal-observer-mode`. It will discover existing processes with an initial `/proc` inventory snapshot, reconcile that inventory every 30 seconds, and use the Linux netlink process connector (`NETLINK_CONNECTOR` / `CN_PROC`) for fork, exec, and exit notifications between scans. Normal mode will collect procfs resource measurements without loading or attaching eBPF programs. Netlink will be the normal lifecycle event source.

When an anomaly is detected, `forensic-observer-mode` will add eBPF monitoring for the affected processes or threads. Aya will load and attach the required programs for the forensic session. Normal discovery, reconciliation, and procfs collection will continue during forensic capture. When all forensic sessions end, their eBPF resources will be released and the observer will return to normal mode. Anomaly rules, capture probes, duration, and resource limits remain to be defined.

```mermaid
stateDiagram-v2
    [*] --> Normal
    Normal: normal-observer-mode
    Normal: Startup procfs inventory + reconciliation every 30 seconds
    Normal: Netlink lifecycle events + procfs measurements
    Normal --> Forensic: Anomaly detected for a process or thread
    Forensic: forensic-observer-mode
    Forensic: Continue normal observation + targeted eBPF capture
    Forensic --> Normal: All forensic sessions ended, release eBPF resources
```

### Process targets

`process_discovery` will subscribe to netlink lifecycle notifications before the startup inventory scan when the connector is available. It will forward scan results and events to `process_worker`, which will deduplicate them by process identity. Reconciliation every 30 seconds will discover missed processes and remove confirmed stale entries. Inventory scans identify targets; their cadence is independent of resource sampling intervals.

`process_worker` will apply selection rules, update `process_registry`, and coordinate collection for the selected targets. The registry will identify each process by PID and start time so PID reuse does not attach an old process's history to a new one. The coordinator will start and stop per-process collection tasks, combine repeated collection requests, and send lifecycle records to the output worker.

Netlink events include thread identifiers and thread-group identifiers, so a thread exit must not be treated as the exit of the whole process. Delayed events must not remove a newer process that reused a PID. If netlink is unavailable or loses events, discovery will continue through procfs reconciliation and report reduced lifecycle coverage; eBPF will remain reserved for anomaly-triggered forensics. Short-lived processes may be missed when events are unavailable or lost. [ADR-0003](docs/architecture/adr/0003-trigger-process-capture-with-bpf-events.md) defines discovery, recovery, and forensic mode boundaries.

### Measurement collection

`collector_worker` tasks will use the registered targets and enabled resource collectors. Configuration will select collection for an individual process, a full pass over the selected process list, system totals, or a combination.

Resource modules such as `collector_cpu`, `collector_memory`, and `collector_io` will provide collection functions. For example, the CPU collector will support system counters from `/proc/stat` and per-PID counters from `/proc/<pid>/stat`. Per-process memory and I/O fields still need to be defined. Network and pressure collection are planned extensions; per-process measurements will require sources that support that attribution, and unavailable measurements will remain distinguishable from zero.

Per-process tasks will use adaptive sampling intervals, bounded jitter, and a shared read concurrency limit. Full-list requests will be coordinated with active tasks to avoid duplicate reads. System totals will be sampled independently when requested.

Collectors will send timestamped observations directly to `output_worker` through channels. A discovery scan identifies targets; a collection pass reads their measurements. A full process snapshot is a pass over the selected targets, not an atomic view of every process or the whole system.

### Output handling

`output_worker` will receive resource observations and lifecycle records, preserving timestamps, process identities, raw values, units, and unavailable measurements. When configured, it will derive CPU utilization and I/O rates from consecutive valid samples. First samples establish baselines; counter resets or identity changes invalidate affected deltas.

`output_model` will expose message types generated from shared Protobuf schemas, and `output_encoding` will encode and decode records using `prost`. `output_storage` will hold encoded records in a planned in-memory ring buffer capped at 500 KB (500,000 bytes), including buffer bookkeeping.

The output worker will route buffered Protobuf records to the configured external collector or local processing consumer. Both will use the same schema. The transport, local consumer interface, batching, retry behavior, buffer overflow policy, channel backpressure, and pending-record handling during shutdown remain to be defined.

### Runtime coordination

`runtime` will launch the workers required by configuration, connect their channels, provide shared concurrency limits, monitor failures, and manage shutdown. `process_worker` will coordinate collection for registered process targets, while `runtime` will start independent system collection when totals are requested. The runtime will start in normal mode and coordinate targeted forensic sessions only after anomaly detection, validating target identities and releasing eBPF resources when sessions end or startup fails.

Startup will follow this order for the enabled workers:

1. `process_*`: start discovery and target coordination, and establish the process registry.
2. `collector_*`: start collection tasks for the registered targets and any requested system totals.
3. `output_*`: initialize output storage and start the worker that prepares, buffers, and delivers Protobuf records.

Collection tasks will wait until the output worker is ready before producing observations. Lifecycle record delivery will also wait for output readiness, while process discovery continues tracking targets.

## Architecture decisions

- [ADR-0001: Collect System Resource Snapshots from procfs](docs/architecture/adr/0001-collect-system-resource-snapshots-from-procfs.md)
- [ADR-0002: Schedule Per-Process Sampling Workers](docs/architecture/adr/0002-schedule-per-process-sampling-workers.md)
- [ADR-0003: Discover Processes with procfs and Netlink; Reserve eBPF for Forensics](docs/architecture/adr/0003-trigger-process-capture-with-bpf-events.md)
- [ADR-0004: Observation Record Format Decision (Protobuf)](docs/architecture/adr/0004-observation-record-format-decision.md)
