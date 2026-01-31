//! The Crew Orchestrator (CrewOrchestrator.md): runs a crew from its charter. It validates the
//! charter against the guardrail defaults and maxima, starts each member's step as a run through the
//! Session Supervisor, and follows two modes. In *coordinated* mode Ordin gives a crew-tool call
//! each round and the orchestrator checks it against the guardrails in fixed code. In *saved steps*
//! mode the orchestrator walks the workflow outline itself, with no coordinator turn per step, and
//! calls Ordin only where the outline asks.
//!
//! It owns the [`Coordinator`] trait (ADR-100), which the Ordin Host implements, and [`CrewHost`],
//! which the programs implement. Tables `crew_runs`, `rounds` and `crew_step_states` live in the
//! Journal's datastore, registered as module `crew_orchestrator` through
//! [`CrewOrchestrator::migrations`]. No environment variable and no configuration key is read here.
//!
//! A crew never continues on its own after a restart: [`CrewOrchestrator::recover`] pauses it and
//! the person presses Continue.
#![forbid(unsafe_code)]

mod coordinated;
mod dispatch;
mod driver;
mod error;
mod guard;
mod orchestrator;
mod plan;
mod saved;
mod store;
mod types;

pub use error::{CrewError, ALL_CODES};
pub use orchestrator::CrewOrchestrator;
pub use types::{
    AgentAvailability, AnsweredBy, CheckVerdict, Coordinator, CoordinatorError, CoordinatorTurn,
    CrewHost, CrewInput, CrewInputValue, CrewMode, CrewOutcome, CrewStart, CrewToolCall, CrewView,
    LimitLowered, MemberAnswer, PathDecision, PersonQuestion, PersonQuestionKind, PlanCheck,
    PlanProblem, StepOutcome, TakeOverReason,
};
