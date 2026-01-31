//! A `ServiceApi` for the tests: every method is generated with `for_each_command!` and hands its
//! input, as JSON, to one handler the test supplies; every call is recorded.

#![allow(dead_code)]

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ordin_service_api::*;
use ordin_types::{BackendId, CommandRequestId, RepairAction};
use serde_json::{json, Value};

pub type Reply = Pin<Box<dyn Future<Output = Result<Value, ServiceError>> + Send>>;
pub type Handler = Arc<dyn Fn(&'static str, Value) -> Reply + Send + Sync>;

pub const BACKEND: &str = "0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b";
pub const REQUEST: &str = "0192a1c0-0d1e-7f20-8a31-425364758697";

/// Every group of the catalogue, sorted (a build with every switch).
pub fn every_group() -> &'static [&'static str] {
    static GROUPS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    GROUPS.get_or_init(|| {
        let mut groups: Vec<&'static str> = CATALOGUE.iter().map(|m| m.module).collect();
        groups.sort_unstable();
        groups.dedup();
        groups
    })
}

pub struct Mock {
    pub handler: Handler,
    pub calls: Mutex<Vec<&'static str>>,
    pub groups: &'static [&'static str],
}

impl Mock {
    pub fn new(handler: impl Fn(&'static str, Value) -> Reply + Send + Sync + 'static) -> Mock {
        Mock {
            handler: Arc::new(handler),
            calls: Mutex::new(Vec::new()),
            groups: every_group(),
        }
    }

    /// A mock whose every method answers `result`.
    pub fn answering(result: Value) -> Mock {
        Mock::new(move |_, _| {
            let result = result.clone();
            Box::pin(async move { Ok(result) })
        })
    }

    pub fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

macro_rules! mock_methods {
    ($( ($module:ident, $verb:ident, $method:ident, $input:ty, $result:ty, $scope:ident, $effect:ident); )*) => {
        #[async_trait::async_trait]
        impl ServiceApi for Mock {
            $(
                async fn $method(&self, _ctx: &CallContext, input: $input) -> Result<$result, ServiceError> {
                    if let Ok(mut calls) = self.calls.lock() {
                        calls.push(stringify!($method));
                    }
                    let value = serde_json::to_value(&input).expect("input serialises");
                    let out = (self.handler)(stringify!($method), value).await?;
                    Ok(serde_json::from_value(out).expect("result has the command's shape"))
                }
            )*

            fn groups(&self) -> &'static [&'static str] {
                self.groups
            }
        }
    };
}

ordin_service_api::for_each_command!(mock_methods);

/// Sentences that name the code, and the Error Table's actions.
pub struct Sentences;

impl ErrorSentences for Sentences {
    fn sentence(&self, code: &str) -> String {
        match code {
            "SVC_INPUT_INVALID" => "ordin couldn't read that request. Try again.".to_owned(),
            other => format!("Sentence for {other}."),
        }
    }

    fn action(&self, code: &str) -> Option<RepairAction> {
        match code {
            "SVC_INPUT_INVALID" => Some(RepairAction::TryAgain),
            _ => None,
        }
    }
}

pub fn backend() -> BackendId {
    BackendId::parse(BACKEND).expect("backend id")
}

pub fn request_id() -> CommandRequestId {
    CommandRequestId::parse(REQUEST).expect("request id")
}

/// An envelope for `command` with `input`, at version 1.0, for this backend.
pub fn envelope(command: &str, input: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "api_version": {"major": 1, "minor": 0},
        "request_id": REQUEST,
        "command": command,
        "backend": BACKEND,
        "input": input,
    }))
    .expect("envelope serialises")
}

pub fn paired() -> CallOrigin {
    CallOrigin::PairedDesktop {
        desktop_pairing_id: ordin_types::DesktopPairingId::new(),
        computer_name: "Asha's laptop".to_owned(),
    }
}

/// `accept` of `body` routed as `routed`, from `origin`, with every group answered.
pub fn accept_with(body: &[u8], routed: &str, origin: CallOrigin) -> Result<Accepted, Refusal> {
    accept(&Incoming {
        body,
        routed_name: routed,
        origin,
        this_backend: backend(),
        groups: every_group(),
    })
}

/// `accept` of `command` with `input` from the local screens.
pub fn accept_local(command: &str, input: Value) -> Result<Accepted, Refusal> {
    accept_with(&envelope(command, input), command, CallOrigin::LocalScreens)
}

/// `handle` of `body` with a deadline `secs` ahead.
pub async fn handle_with(
    api: &Mock,
    body: &[u8],
    routed: &str,
    origin: CallOrigin,
    secs: u64,
) -> (HandledReply, Value) {
    let reply = handle(
        api,
        &Sentences,
        Incoming {
            body,
            routed_name: routed,
            origin,
            this_backend: backend(),
            groups: api.groups,
        },
        Instant::now() + Duration::from_secs(secs),
        tokio_util::sync::CancellationToken::new(),
    )
    .await;
    let json = if reply.json.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&reply.json).expect("reply is JSON")
    };
    (reply, json)
}

/// `handle` of `command` with `input` from the local screens.
pub async fn handle_local(api: &Mock, command: &str, input: Value) -> (HandledReply, Value) {
    handle_with(
        api,
        &envelope(command, input),
        command,
        CallOrigin::LocalScreens,
        10,
    )
    .await
}

/// A module error as a program builds it.
pub fn module_error(code: &str) -> ServiceError {
    ServiceError {
        code: code.to_owned(),
        message: format!("Sentence for {code}."),
        action: None,
        reference: None,
    }
}
