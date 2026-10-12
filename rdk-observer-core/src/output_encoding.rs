//! Observation record encoding.
//!
//! Following ADR-0004, this module will provide functions used by `output_worker`
//! to map prepared system, process, and lifecycle observations to generated
//! Protobuf message types in `output_model` and encode/decode them with `prost`.
//! External and local consumers will use the same format.
//!
//! Mappings will preserve timestamps, identities, units, raw and derived values,
//! and explicit presence so unavailable values remain distinguishable from zero.
//! Full-list records will retain the identity and timestamp of each sampled
//! target. Binary framing and batching will be defined by the delivery protocol.
