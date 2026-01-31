//! The 2C domain entities (part 1), one module per 2C section. Reached by module path only.
//!
//! Every submodule but `agent_profile` is re-exported here. SPEC-GAP-7: the agent-configuration types
//! that AgentConfiguration.md places in `agent_profile` reuse two names the 2C domain already has
//! (`Condition` in `runs`, `Authority` in `agents`), so a glob re-export of `agent_profile` would make
//! both ambiguous. Those types are reached at `ordin_types::domain::agent_profile::…`, the path
//! SharedTypes.md *Module paths* gives them.

pub mod agent_profile;
pub mod agents;
pub mod email;
pub mod engineering;
pub mod handover;
pub mod identity;
pub mod places;
pub mod runs;
pub mod server;

pub use agents::*;
pub use email::*;
pub use engineering::*;
pub use handover::*;
pub use identity::*;
pub use places::*;
pub use runs::*;
pub use server::*;
