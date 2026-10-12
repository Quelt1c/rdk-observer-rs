//! In-memory observation record buffering.
//!
//! This module will provide functions to store encoded Protobuf records in an
//! in-memory ring buffer used by `output_worker`. Records will be queued for
//! delivery to the configured external or local consumer. The buffer will
//! occupy at most 500 KB (500,000 bytes), including record data and bookkeeping.
//! Functions will support adding and retrieving records within that limit.
//!
//! Handling a full buffer and oversized records remains to be defined.
//! Serialization will belong to `output_encoding`, while `output_worker` will
//! route records to the selected consumer. Record removal will follow the
//! delivery policy once it is defined.
