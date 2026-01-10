//! The versioned command catalogue (ServiceApi.md *Command catalogue, version 1.0*; Internal Logic 1).
//!
//! The catalogue is written once as data, in `rows.rs`, through `service_catalogue!`, which generates
//! the `ServiceApi` trait (one method per row), `CATALOGUE` in row order, `command()`, `dispatch()`,
//! the input check `accept` calls, the callback macro `for_each_command!` and the schema registry
//! (`CommandMeta.input_schema` / `result_schema`). Name, IPC name and REST path are derived from the
//! row's module and verb, so the transports cannot drift (SA-API-01).

use serde_json::Value;

use crate::api::{CommandInput, InputRejection};
use crate::commands::rules;
use crate::wire::ServiceError;

/// The schema of `T`, as the registry hands it to the generator.
pub(crate) fn schema_of<T: schemars::JsonSchema>(
    generator: &mut schemars::SchemaGenerator,
) -> schemars::Schema {
    generator.subschema_for::<T>()
}

/// An error `dispatch` raises itself; `handle` fills the sentence and the action from `ErrorSentences`.
pub(crate) fn unfilled(code: &str) -> ServiceError {
    ServiceError {
        code: code.to_owned(),
        message: String::new(),
        action: None,
        reference: None,
    }
}

/// The JSON Pointer of a `serde_path_to_error` path, under `/input`.
fn input_pointer(path: &serde_path_to_error::Path) -> String {
    use serde_path_to_error::Segment;
    let mut out = String::from("/input");
    for segment in path.iter() {
        match segment {
            Segment::Seq { index } => out.push_str(&format!("/{index}")),
            Segment::Map { key } => {
                out.push('/');
                out.push_str(&key.replace('~', "~0").replace('/', "~1"));
            }
            Segment::Enum { .. } | Segment::Unknown => {}
        }
    }
    out
}

/// Steps 8–10 of `accept` for one input type: read it with `serde_path_to_error`, run its `check`,
/// and re-serialise the normalised value.
pub(crate) fn checked<I: CommandInput + serde::Serialize>(
    input: &Value,
) -> Result<Value, InputRejection> {
    let mut typed: I = serde_path_to_error::deserialize(input).map_err(|e| {
        let mut at = input_pointer(e.path());
        // An unknown variant of a `kind`-tagged object is the object's fault, not its tag's: the
        // pointer names the object (ADR-110 scenario: `/input/item`).
        if e.inner().to_string().starts_with("unknown variant") {
            if let Some(object) = at.strip_suffix("/kind") {
                at = object.to_owned();
            }
        }
        InputRejection {
            code: rules::INPUT_INVALID,
            at,
        }
    })?;
    typed.check()?;
    serde_json::to_value(&typed).map_err(|_| InputRejection {
        code: rules::INPUT_INVALID,
        at: "/input".to_owned(),
    })
}

macro_rules! service_catalogue {
    (
        $d:tt;
        $(
            $module:ident $verb:ident $method:ident => $input:ty => $result:ty, $scope:ident, $effect:ident, $budget:ident;
        )*
    ) => {
        /// Implemented once by each program (desktop, ordin server); one method per command of the
        /// catalogue, in catalogue order. Each method returns the command's result or a `ServiceError`
        /// whose message the implementation built with `ErrorSentences`.
        #[async_trait::async_trait]
        pub trait ServiceApi: Send + Sync + 'static {
            $(
                async fn $method(
                    &self,
                    ctx: &$crate::api::CallContext,
                    input: $input,
                ) -> Result<$result, $crate::wire::ServiceError>;
            )*

            /// The `CommandMeta.module` names this implementation answers, sorted. Fixed for the life
            /// of the process; `service.hello` reports the same list in `ServiceHello.groups`
            /// (SVC-RL-1, amended January 2026, ADR-114).
            fn groups(&self) -> &'static [&'static str];
        }

        /// The position of each row in `CATALOGUE`.
        #[allow(non_camel_case_types, clippy::enum_variant_names)]
        #[derive(Clone, Copy)]
        enum Row {
            $( $method, )*
        }

        /// The catalogue, in table order.
        pub static CATALOGUE: &[$crate::api::CommandMeta] = &[
            $(
                $crate::api::CommandMeta {
                    name: concat!(stringify!($module), ".", stringify!($verb)),
                    module: stringify!($module),
                    verb: stringify!($verb),
                    ipc_name: stringify!($method),
                    rest_path: concat!("/v1/", stringify!($module), "/", stringify!($verb)),
                    scope: $crate::api::Scope::$scope,
                    effect: $crate::api::Effect::$effect,
                    budget: $crate::api::Budget::$budget,
                    since: $crate::wire::ServiceApiVersion { major: 1, minor: 0 },
                    input_schema: $crate::catalogue::schema_of::<$input>,
                    result_schema: $crate::catalogue::schema_of::<$result>,
                },
            )*
        ];

        /// Looks a command up by its dotted name.
        pub fn command(name: &str) -> Option<&'static $crate::api::CommandMeta> {
            let row = match name {
                $( concat!(stringify!($module), ".", stringify!($verb)) => Row::$method, )*
                _ => return None,
            };
            CATALOGUE.get(row as usize)
        }

        /// Calls the trait method a command names, with input already checked.
        pub async fn dispatch(
            api: &dyn ServiceApi,
            ctx: &$crate::api::CallContext,
            meta: &'static $crate::api::CommandMeta,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, $crate::wire::ServiceError> {
            dispatch_boxed(api, ctx, meta, input).await
        }

        /// One boxed future per row, so the future of `dispatch` stays small: a single `async`
        /// match over every row would hold every row's locals at once.
        fn dispatch_boxed<'a>(
            api: &'a dyn ServiceApi,
            ctx: &'a $crate::api::CallContext,
            meta: &'static $crate::api::CommandMeta,
            input: serde_json::Value,
        ) -> futures::future::BoxFuture<'a, Result<serde_json::Value, $crate::wire::ServiceError>> {
            match meta.ipc_name {
                $(
                    stringify!($method) => Box::pin(async move {
                        let input: $input = serde_json::from_value(input)
                            .map_err(|_| $crate::catalogue::unfilled("SVC_INPUT_INVALID"))?;
                        let result = api.$method(ctx, input).await?;
                        serde_json::to_value(result)
                            .map_err(|_| $crate::catalogue::unfilled("SVC_INTERNAL"))
                    }),
                )*
                _ => Box::pin(async { Err($crate::catalogue::unfilled("SVC_COMMAND_UNKNOWN")) }),
            }
        }

        /// Steps 8–10 of `accept` for the command `meta` names.
        pub(crate) fn check_input(
            meta: &$crate::api::CommandMeta,
            input: &serde_json::Value,
        ) -> Result<serde_json::Value, $crate::api::InputRejection> {
            match meta.ipc_name {
                $( stringify!($method) => $crate::catalogue::checked::<$input>(input), )*
                _ => Err($crate::api::InputRejection {
                    code: "SVC_COMMAND_UNKNOWN",
                    at: String::new(),
                }),
            }
        }

        /// Invokes `$callback! { (module, verb, method, Input, Result, scope, effect); … }` once with
        /// every catalogue row. The Command Bridge generates its Tauri commands and capability list
        /// with it; the Remote API needs only `CATALOGUE`.
        #[macro_export]
        macro_rules! for_each_command {
            ($d callback:ident) => {
                $d callback! {
                    $( ($module, $verb, $method, $input, $result, $scope, $effect); )*
                }
            };
        }
    };
}

mod rows;

pub(crate) use rows::check_input;
pub use rows::{command, dispatch, ServiceApi, CATALOGUE};

/// Commands only a person may send: no MCP Front tool, workflow step kind or block action names them,
/// so no run, crew member, workflow step or Ordin can reach them (ADR-110 *Adopting is the person's
/// alone*; ADR-111; SA-SVC-KA-01; SVC-RL-4). The Architecture Test asserts it over the sources.
// spec: ordin-desktop/components/ServiceApi#SVC-RL-4
// spec: ordin-desktop/components/ServiceApi#SA-SVC-KA-01
pub const PERSON_ONLY_COMMANDS: &[&str] = &[
    "team_knowledge.adopt",
    "team_knowledge.adopt_again",
    "team_knowledge.keep_as_is",
    "memory.correct",
    "memory.forget",
    "memory.remember_everywhere",
    "memory.keep",
    "memory.correct_link",
    "memory.forget_link",
    "memory.forget_thing",
    "memory.same_as",
    "knowledge_agents.add",
    "knowledge_agents.remove",
    "setup.update_server",
    "setup.install_update",
];

/// The command groups (`CommandMeta.module` names) every build of a program answers (ADR-114, *Command
/// groups by build switch*, row *none*). *Chosen here*: the spec gives the table, not its Rust form.
pub const ALWAYS_ANSWERED_GROUPS: &[&str] = &[
    "service",
    "app",
    "settings",
    "files",
    "home",
    "search",
    "workspaces",
    "craft",
    "push",
    "confluence",
    "jira",
    "sharepoint",
    "ordin",
    "voice",
    "workflows",
    "dashboards",
    "runs",
    "crews",
    "approval",
    "journal",
    "artefacts",
    "reports",
    "plans",
    "history",
    "diagnostics",
    "profile",
    "remote_agents",
    "knowledge",
    "memory",
    "catalogue",
    "checker",
    "agents",
    "projects",
    "usage",
    "identity",
    "setup",
    "localisation",
    "brand",
    "servers",
];

/// The groups answered only by a build compiled with a build switch (Cargo feature of the programs),
/// as `(switch, group)` (ADR-114, *Command groups by build switch*). The switches `sandbox` and
/// `copilot` add no group.
pub const GROUPS_BY_SWITCH: &[(&str, &str)] = &[
    ("dictation", "dictation"),
    ("mail", "mail"),
    ("slack", "slack"),
    ("handover", "handover"),
    ("authoring", "authoring"),
    ("team-knowledge", "team_knowledge"),
    ("telegram", "telegram"),
    ("hosted-agents", "hosted"),
    ("knowledge-agents", "knowledge_agents"),
];
