//! In-memory observation record buffering.
//!
//! This module will provide functions to store encoded Protobuf records in an
//! in-memory ring buffer used by `output_worker`. Records will be available for
//! subsequent consumption through an interface still to be defined. The buffer
//! will occupy at most 500 KB (500,000 bytes), including record data and bookkeeping.
//! Functions will support adding and retrieving records within that limit.
//!
//! Handling a full buffer and oversized records remains to be defined.
//! Serialization will belong to `output_encoding`; `output_worker` will prepare
//! and store records. The consuming component and record removal policy remain
//! to be defined separately.
