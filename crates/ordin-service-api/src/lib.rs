//! The one definition of what ordin's screens can ask a backend and what a backend tells them
//! (ServiceApi.md; ADR-096 item 2).
//!
//! The versioned command catalogue and the `ServiceApi` trait with one method per command (ADR-100
//! item 1), the command and event envelopes, the event catalogue and its decoding from the Journal's
//! records, the block catalogue with the check every block passes before it is drawn (ADR-028,
//! ADR-067, ADR-068, ADR-070), and the generator of the JSON Schemas and TypeScript types the screens
//! module is written against (SA-API-02). Both transports — the Command Bridge on Tauri IPC and the
//! Remote API over HTTPS — call the same envelope functions here, so a command is checked the same way
//! whichever transport carries it. The crate does no I/O except the generator binary.
#![forbid(unsafe_code)]

// The catalogue's rows name their types by absolute path (`::ordin_service_api::commands::…`), so
// that `for_each_command!` hands other crates paths that resolve there too.
extern crate self as ordin_service_api;

/// Re-exported so the paths `for_each_command!` hands out resolve in any crate.
pub use ordin_types;

pub mod api;
pub mod blocks;
pub mod catalogue;
pub mod commands;
pub mod envelope;
pub mod error;
pub mod events;
pub mod pairing;
pub mod schema;
pub mod typescript;
pub mod wire;

pub use api::{
    Budget, CallContext, CallOrigin, CommandInput, CommandMeta, Done, Effect, ErrorSentences,
    InputRejection, Scope,
};
pub use catalogue::{
    command, dispatch, ServiceApi, ALWAYS_ANSWERED_GROUPS, CATALOGUE, GROUPS_BY_SWITCH,
    PERSON_ONLY_COMMANDS,
};
pub use envelope::{
    accept, handle, Accepted, HandledReply, Incoming, Refusal, MAX_BODY_BYTES,
    MAX_EVENT_DATA_BYTES, MAX_TEXT_SAVE_BODY_BYTES,
};
pub use error::ALL_CODES;
pub use events::{encode_for_transport, EventError, ServiceEvent, TransportEvent};
pub use schema::json_schemas;
pub use wire::{
    CommandEnvelope, CommandName, CommandOutcome, CommandReply, EventEnvelope, ServiceApiVersion,
    ServiceError,
};
