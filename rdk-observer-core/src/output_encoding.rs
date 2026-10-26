//! Observation record encoding.
//!
//! This module will map records from `output_model` to messages generated from
//! the shared Protobuf schema and encode/decode them using `prost`.
//! Conversions will preserve process identities, sampling timestamps, optional
//! measurements, and collections of thread identities, metadata, and measurements.
//! Unavailable values will remain distinguishable from measured zero.
//! The delivery protocol will define record framing and batching.
