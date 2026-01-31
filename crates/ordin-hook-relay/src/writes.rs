//! Telling the File Service what a call may write, before it runs (FileService.md Internal Logic 6a).
//!
//! The relay is the one place every harness tool call passes before it runs, and the only one that
//! sees it: a harness's own file tools write straight to the disk. The File Service is a sibling, so
//! the relay holds it behind [`AgentWrites`], which the program implements over it.
// spec: ordin-desktop/components/HookRelay#SA-RECON-05

use std::path::PathBuf;

use ordin_types::wire::rules::ToolOrigin;
use ordin_types::RunId;
use serde_json::Value;

/// The harness file tools that write, and the argument naming the file (RulesEngine.md H.1).
const FILE_WRITERS: [(&str, &str); 6] = [
    ("Write", "file_path"),
    ("Edit", "file_path"),
    ("MultiEdit", "file_path"),
    ("NotebookEdit", "notebook_path"),
    ("create", "path"),
    ("edit", "path"),
];

/// Built-in tools that write nothing themselves. A sub-agent's own calls come through the hook.
const NOT_WRITERS: [&str; 8] = [
    "Read", "Grep", "Glob", "LS", "view", "Task", "task", "Agent",
];

/// What a tool call may write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteTarget {
    /// One absolute path.
    File(PathBuf),
    /// Anything: the call's writes cannot be told from its arguments.
    Anywhere,
}

/// The program's bridge to the File Service. `open` runs before the call does, and an `Err` refuses
/// the call, so a write cannot reach the disk with no author.
#[async_trait::async_trait]
pub trait AgentWrites: Send + Sync {
    /// The claim to settle after the call, if there is one.
    async fn open(
        &self,
        run_id: RunId,
        tool: &str,
        target: WriteTarget,
    ) -> Result<Option<String>, AgentWriteError>;
    /// The call has ended.
    async fn settle(&self, claim: &str) -> Result<(), AgentWriteError>;
    /// The run's session has closed.
    async fn run_ended(&self, run_id: RunId) -> Result<(), AgentWriteError>;
    /// Whether this implementation records anything. Only [`NoPlaces`] does not.
    fn attributes_writes(&self) -> bool {
        true
    }
}

/// The claim could not be recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentWriteError;

/// For a program with no places: nothing an agent writes can be published from it, so there is
/// nothing to attribute. Wiring this into a program with places turns attribution off, so such a
/// program checks [`crate::HookRelay::attributes_writes`] at start.
pub struct NoPlaces;

#[async_trait::async_trait]
impl AgentWrites for NoPlaces {
    async fn open(
        &self,
        _: RunId,
        _: &str,
        _: WriteTarget,
    ) -> Result<Option<String>, AgentWriteError> {
        Ok(None)
    }
    async fn settle(&self, _: &str) -> Result<(), AgentWriteError> {
        Ok(())
    }
    async fn run_ended(&self, _: RunId) -> Result<(), AgentWriteError> {
        Ok(())
    }
    fn attributes_writes(&self) -> bool {
        false
    }
}

/// What the call may write; `None` when it writes nothing outside ordin's own services, which record
/// their own versions. Anything not known to be narrower is `Anywhere`, including a relative path,
/// which the relay has no folder to resolve against, and a path with `..`, which names one file
/// lexically and another through a symbolic link.
pub(crate) fn target_of(tool: &str, origin: &ToolOrigin, arguments: &Value) -> Option<WriteTarget> {
    match origin {
        ToolOrigin::OrdinMcpFront { .. } => None,
        ToolOrigin::PackageMcpServer { .. } => Some(WriteTarget::Anywhere),
        ToolOrigin::BuiltIn => {
            if NOT_WRITERS.contains(&tool) {
                return None;
            }
            let path = FILE_WRITERS
                .iter()
                .find(|(name, _)| *name == tool)
                .and_then(|(_, argument)| arguments.get(argument))
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .filter(|path| {
                    path.is_absolute()
                        && !path
                            .components()
                            .any(|part| matches!(part, std::path::Component::ParentDir))
                });
            Some(path.map_or(WriteTarget::Anywhere, WriteTarget::File))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// An absolute path literal on this platform: on Windows a drive goes before the root.
    #[cfg(unix)]
    macro_rules! abs {
        ($path:literal) => {
            $path
        };
    }
    #[cfg(windows)]
    macro_rules! abs {
        ($path:literal) => {
            concat!("C:", $path)
        };
    }

    #[test]
    fn a_call_is_read_for_what_it_may_write() {
        let built_in = ToolOrigin::BuiltIn;
        let file = |path: &str| Some(WriteTarget::File(PathBuf::from(path)));
        assert_eq!(
            target_of("Write", &built_in, &json!({"file_path": abs!("/p/a.md")})),
            file(abs!("/p/a.md"))
        );
        assert_eq!(
            target_of(
                "NotebookEdit",
                &built_in,
                &json!({"notebook_path": abs!("/p/a")})
            ),
            file(abs!("/p/a"))
        );
        assert_eq!(
            target_of("create", &built_in, &json!({"path": abs!("/p/a")})),
            file(abs!("/p/a"))
        );
        assert_eq!(target_of("Read", &built_in, &json!({})), None);
        let wide = Some(WriteTarget::Anywhere);
        assert_eq!(
            target_of("Bash", &built_in, &json!({"command": "ls"})),
            wide
        );
        assert_eq!(target_of("NewTool", &built_in, &json!({})), wide);
        assert_eq!(
            target_of("Write", &built_in, &json!({"file_path": "a.md"})),
            wide
        );
        assert_eq!(target_of("Write", &built_in, &json!({})), wide);
        for path in [
            abs!("/p/nope/../a.md"),
            abs!("/p/link/../a.md"),
            abs!("/p/.."),
        ] {
            assert_eq!(
                target_of("Write", &built_in, &json!({"file_path": path})),
                wide,
                "{path}"
            );
        }
        let package = ToolOrigin::PackageMcpServer {
            server: "x".to_owned(),
        };
        assert_eq!(target_of("anything", &package, &json!({})), wide);
        let front = ToolOrigin::OrdinMcpFront {
            server: "x".to_owned(),
        };
        assert_eq!(target_of("write_file", &front, &json!({})), None);
    }
}
