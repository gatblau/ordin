//! Craft tool definitions (CraftToolkit.md *Craft tool definitions*): the shipped description of
//! each app's tools. A tool not described here is never offered, so the check fails closed.

use std::collections::HashSet;

use ordin_types::domain::places::CopyFormat;
use serde::Deserialize;

/// The `craft-tools.json` shipped inside this build (02:757). C-07, C-21 and C-28 are unconfirmed
/// for every app, so it lists none.
/// Nothing else hands the bundled text to the program.
pub const BUNDLED_CRAFT_TOOLS: &str = include_str!("../craft-tools.json");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftToolDefinitions {
    pub format: u32,
    pub apps: Vec<CraftAppDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftAppDefinition {
    pub app: String,
    /// Cargo SemVer requirement, "^0.7".
    pub versions: String,
    /// Binary name, "photocraft-cli".
    pub cli: String,
    /// How to open the app's window.
    pub window: AppWindow,
    pub tools: Vec<CraftToolDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AppWindow {
    /// "PhotoCraft.app"
    pub macos_bundle: Option<String>,
    /// "PhotoCraft.exe"
    pub windows_exe: Option<String>,
    /// "photocraft"
    pub linux_exe: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftToolDefinition {
    /// The app's MCP tool name.
    pub tool: String,
    /// One line (ADR-062).
    pub description: String,
    /// Arguments holding file paths, with their role. Any other argument is passed unchanged.
    pub path_args: Vec<PathArg>,
    /// Set on the conversion tools of ADR-048: which copy format this tool makes from Markdown.
    pub converts_markdown_to: Option<CopyFormat>,
    /// Set on a reverse-conversion tool: which copy format this tool turns into Markdown
    /// (SA-RECON-03; empty for every app until C-13 is confirmed).
    #[serde(default)]
    pub converts_to_markdown_from: Option<CopyFormat>,
    /// For a slides conversion: the argument that sets a first-slide footer, when the tool has one.
    pub footer_arg: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PathArg {
    pub name: String,
    pub role: PathRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathRole {
    Input,
    Output,
}

impl CraftToolDefinitions {
    /// Parses the bundled file. `None` when the file is broken, which the build's test catches.
    pub fn bundled() -> Option<CraftToolDefinitions> {
        serde_json::from_str(BUNDLED_CRAFT_TOOLS).ok()
    }
}

/// A definition that passed Internal Logic 1.2.
#[derive(Debug, Clone)]
pub(crate) struct UsableDefinition {
    pub(crate) definition: CraftAppDefinition,
    pub(crate) versions: semver::VersionReq,
}

/// The apps of `definitions` that pass Internal Logic 1.2; the others are skipped and logged
/// `definition_invalid {app}`. A format other than 1 leaves none, because no entry can be read.
pub(crate) fn usable(definitions: &CraftToolDefinitions) -> Vec<UsableDefinition> {
    definitions
        .apps
        .iter()
        .filter_map(|app| match check(definitions.format, app) {
            Some(versions) => Some(UsableDefinition {
                definition: app.clone(),
                versions,
            }),
            None => {
                tracing::warn!(component = "craft", app = %app.app, "definition_invalid");
                None
            }
        })
        .collect()
}

fn check(format: u32, app: &CraftAppDefinition) -> Option<semver::VersionReq> {
    if format != 1 {
        return None;
    }
    let versions = semver::VersionReq::parse(&app.versions).ok()?;
    let mut conversions = HashSet::new();
    for tool in &app.tools {
        let mut names = HashSet::new();
        if !tool.path_args.iter().all(|arg| names.insert(&arg.name)) {
            return None;
        }
        if let Some(format) = tool.converts_markdown_to {
            if !conversions.insert(format!("{format:?}")) {
                return None;
            }
        }
    }
    Some(versions)
}

/// The definition of `app` whose `versions` covers `version`.
pub(crate) fn matching<'a>(
    definitions: &'a [UsableDefinition],
    app: &str,
    version: &str,
) -> Option<&'a CraftAppDefinition> {
    let version = semver::Version::parse(version).ok()?;
    definitions
        .iter()
        .find(|d| d.definition.app == app && d.versions.matches(&version))
        .map(|d| &d.definition)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(versions: &str, tools: Vec<CraftToolDefinition>) -> CraftAppDefinition {
        CraftAppDefinition {
            app: "photocraft".to_owned(),
            versions: versions.to_owned(),
            cli: "photocraft-cli".to_owned(),
            window: AppWindow {
                macos_bundle: None,
                windows_exe: None,
                linux_exe: None,
            },
            tools,
        }
    }

    fn tool(paths: &[&str], converts: Option<CopyFormat>) -> CraftToolDefinition {
        CraftToolDefinition {
            tool: "resize".to_owned(),
            description: "Resizes an image.".to_owned(),
            path_args: paths
                .iter()
                .map(|name| PathArg {
                    name: (*name).to_owned(),
                    role: PathRole::Input,
                })
                .collect(),
            converts_markdown_to: converts,
            converts_to_markdown_from: None,
            footer_arg: None,
        }
    }

    /// C-07, C-21 and C-28 are unconfirmed: the shipped file parses, passes the start checks and
    /// describes no app. A defect in it fails here, in the build's test (Internal Logic 1.2).
    #[test]
    fn the_bundled_file_is_valid_and_lists_no_app_until_confirmed() {
        let definitions = CraftToolDefinitions::bundled().expect("craft-tools.json parses");
        assert_eq!(definitions.format, 1);
        assert_eq!(usable(&definitions).len(), definitions.apps.len());
        assert!(definitions.apps.is_empty());
    }

    #[test]
    fn a_defective_entry_is_skipped() {
        let word = Some(CopyFormat::Word);
        for (format, bad) in [
            (2, app("^0.7", vec![])),
            (1, app("not a requirement", vec![])),
            (1, app("^0.7", vec![tool(&["a", "a"], None)])),
            (1, app("^0.7", vec![tool(&[], word), tool(&[], word)])),
        ] {
            let definitions = CraftToolDefinitions {
                format,
                apps: vec![bad],
            };
            assert!(usable(&definitions).is_empty());
        }
    }

    #[test]
    fn an_unknown_key_is_refused_and_a_version_range_selects_the_entry() {
        let text = r#"{"format":1,"apps":[],"extra":1}"#;
        assert!(serde_json::from_str::<CraftToolDefinitions>(text).is_err());
        let definitions = CraftToolDefinitions {
            format: 1,
            apps: vec![app("^0.7", vec![])],
        };
        let usable = usable(&definitions);
        assert!(matching(&usable, "photocraft", "0.7.3").is_some());
        assert!(matching(&usable, "photocraft", "0.8.0").is_none());
        assert!(matching(&usable, "photocraft", "not a version").is_none());
        assert!(matching(&usable, "wordcraft", "0.7.3").is_none());
    }
}
