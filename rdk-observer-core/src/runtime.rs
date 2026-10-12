//! Task orchestration, scheduling, and shutdown.
//!
//! This module will receive configuration built from input parameters and start
//! the required Tokio workers. Process monitoring will use `process_discovery`
//! and `process_worker` to establish and maintain targets in `process_registry`.
//! Available event readers will be attached before initial discovery scans.
//!
//! Enabled workers will start in this order:
//!
//! 1. `process_*`: start discovery and coordination to establish registered targets.
//! 2. `collector_*`: start process collection tasks and requested system collection.
//! 3. `output_*`: initialize storage and start observation preparation and delivery.
//!
//! Collection tasks will wait for output readiness before producing observations.
//! Lifecycle record delivery will also wait for output readiness while discovery
//! continues tracking targets.
//!
//! `process_worker` will coordinate collection for registered targets, using
//! `collector_worker` tasks for individual processes or full-list collection
//! requests. The runtime will start independent system collection when totals
//! are requested. Collection will use only the enabled resource types.
//!
//! Measurements from collection tasks and lifecycle records from the process
//! coordinator will be sent to a shared `output_worker` through channels. It
//! will prepare Protobuf records, buffer them through `output_storage`, and route
//! them to the configured external or local consumer.
//!
//! The runtime will connect workers, provide shared concurrency limits, monitor
//! failures, and manage startup and shutdown. Pending record handling and
//! failure policies remain to be defined.
