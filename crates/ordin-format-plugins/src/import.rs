//! The part builder the three read plug-ins share (FormatPlugins.md *Internal Logic* 5, 7, 8).
//!
//! Every part an import creates has `apiVersion: ai.enterprise/v1`, the kind's first authored
//! spelling, `owner: "imported"` and `status: draft`. A Markdown body is copied as bytes.

use std::collections::{BTreeMap, BTreeSet};

use ordin_agent_model::{
    content_digest, is_task_input_name, kind_info, mint_identifier, render_manifest_yaml,
    render_part, AmError, Audience, Manifest, Metadata, ModelPart, PartFiles, PartKind,
    RenderedFile, Urn, YamlValue, API_VERSION, IMPORTED_TERM,
};
use ordin_types::yaml::YamlNode;

use crate::error::FormatError;
use crate::front::{self, default_name, file_name, Front};
use crate::guard::MAX_FILE_BYTES;
use crate::types::{MappingItem, UnmappedReason};

/// Where an id comes from.
pub(crate) enum IdSpec<'a> {
    /// Minted from a slug under a category.
    Mint {
        category: Option<&'a str>,
        slug: &'a str,
    },
    /// Fixed by the caller (the system role of a workspace).
    Fixed(String),
}

/// One Markdown file that becomes one part.
pub(crate) struct MdSpec<'a> {
    pub kind: PartKind,
    pub path: &'a str,
    pub bytes: &'a [u8],
    pub id: IdSpec<'a>,
    /// The slug the default name derives from.
    pub default_slug: &'a str,
    /// Front-matter keys the caller reads itself (not reported as unknown).
    pub known_keys: &'a [&'a str],
    /// Sibling files of the part, keyed relative to its directory.
    pub extras: BTreeMap<String, Vec<u8>>,
    /// True when the file has no front matter by definition (the whole text is the body).
    pub whole_text: bool,
    /// True for an APM prompt: its `argument-hint` and `input` are read by FormatPlugins.md FP-TF-2.
    pub prompt_inputs: bool,
}

/// What `md_part` created.
pub(crate) struct Created {
    pub id: String,
    pub name: String,
    pub description: String,
    pub front: Vec<(String, YamlNode)>,
}

/// The identifiers a workspace profile references.
#[derive(Debug, Clone, Default)]
pub(crate) struct Refs {
    pub rules: Vec<String>,
    pub skills: Vec<String>,
    pub commands: Vec<String>,
    pub contexts: Vec<String>,
    pub modes: Vec<String>,
    pub tools: Vec<String>,
    pub system_role: Option<String>,
}

impl Refs {
    /// `self` followed by the entries of `other` that `self` lacks.
    pub fn merged(&self, other: &Refs) -> Refs {
        fn union(a: &[String], b: &[String]) -> Vec<String> {
            let mut out = a.to_vec();
            for id in b {
                if !out.contains(id) {
                    out.push(id.clone());
                }
            }
            out
        }
        Refs {
            rules: union(&self.rules, &other.rules),
            skills: union(&self.skills, &other.skills),
            commands: union(&self.commands, &other.commands),
            contexts: union(&self.contexts, &other.contexts),
            modes: union(&self.modes, &other.modes),
            tools: union(&self.tools, &other.tools),
            system_role: other
                .system_role
                .clone()
                .or_else(|| self.system_role.clone()),
        }
    }
}

/// Renders the file of a part. The kinds with no file convention in the Agent Model that an import
/// still creates get the format's own placement: an OperatingMode is a Markdown rule under
/// `rules/modes/`, a ToolBinding a YAML file under `tool-bindings/`.
// OperatingMode and ToolBinding have no `FileConvention` in the Agent Model, so `render_part`
// refuses them with AM_NO_FILE_CONVENTION, yet the import steps must create them. The placement
// used here follows the directories the kind table gives (`rules/modes/...`,
// `tool-bindings/...`): an OperatingMode as a Markdown file so a chat mode's text is its body, a
// ToolBinding as YAML.
pub(crate) fn render_file(
    kind: PartKind,
    manifest: &Manifest,
    body: Option<&str>,
) -> Result<RenderedFile, AmError> {
    match render_part(kind, manifest, body) {
        Err(AmError::NoFileConvention { .. }) => fallback_file(kind, manifest, body),
        other => other,
    }
}

fn fallback_file(
    kind: PartKind,
    manifest: &Manifest,
    body: Option<&str>,
) -> Result<RenderedFile, AmError> {
    let id = &manifest.metadata.id;
    let urn = Urn::parse(id).ok_or_else(|| AmError::IdInvalid { id: id.clone() })?;
    let mut path = format!("models/@{}/{}", urn.publisher, urn.kind_dir);
    for category in &urn.categories {
        path.push('/');
        path.push_str(category);
    }
    let yaml = render_manifest_yaml(manifest);
    let body = body.unwrap_or("");
    match kind {
        PartKind::OperatingMode => {
            let mut text = format!("---\n{yaml}---\n{body}");
            if !body.is_empty() && !body.ends_with('\n') {
                text.push('\n');
            }
            Ok(RenderedFile {
                path: format!("{path}/{}.md", urn.slug),
                content: text.into_bytes(),
            })
        }
        PartKind::ToolBinding => Ok(RenderedFile {
            path: format!("{path}/{}.yaml", urn.slug),
            content: yaml.into_bytes(),
        }),
        _ => Err(AmError::NoFileConvention { kind }),
    }
}

fn part_invalid(identifier: &str, error: &AmError) -> FormatError {
    FormatError::PartInvalid {
        identifier: identifier.to_owned(),
        code: error.code().to_owned(),
    }
}

/// A part from its manifest, optional Markdown body and sibling files.
pub(crate) fn make_part(
    kind: PartKind,
    manifest: Manifest,
    body: Option<&str>,
    extras: BTreeMap<String, Vec<u8>>,
) -> Result<ModelPart, FormatError> {
    let file =
        render_file(kind, &manifest, body).map_err(|e| part_invalid(&manifest.metadata.id, &e))?;
    let mut files = BTreeMap::new();
    files.insert(file_name(&file.path).to_owned(), file.content);
    for (name, bytes) in extras {
        files.entry(name).or_insert(bytes);
    }
    let files = PartFiles(files);
    Ok(ModelPart {
        kind,
        content_digest: content_digest(&files),
        manifest,
        manifest_path: file.path,
        files,
    })
}

/// The first authored spelling of a kind.
fn authored(kind: PartKind) -> String {
    kind_info(kind)
        .authored_spellings
        .first()
        .map(|s| (*s).to_owned())
        .unwrap_or_default()
}

fn metadata(id: &str, name: &str, description: &str, version: &str) -> Metadata {
    Metadata {
        id: id.to_owned(),
        name: name.to_owned(),
        version: version.to_owned(),
        owner: "imported".to_owned(),
        loading: None,
        description: description.to_owned(),
        category: String::new(),
        class: String::new(),
        audience: Audience::default(),
        use_cases: Vec::new(),
        status: Some("draft".to_owned()),
        supersedes: None,
        extra: BTreeMap::new(),
    }
}

fn str_seq(ids: &[String]) -> YamlValue {
    YamlValue::Seq(ids.iter().map(|id| YamlValue::Str(id.clone())).collect())
}

/// Mints an identifier; an OperatingMode sits under `rules:modes:imported`.
fn mint_with(
    kind: PartKind,
    publisher: &str,
    category: Option<&str>,
    slug: &str,
) -> Result<String, AmError> {
    if kind == PartKind::OperatingMode {
        // `mint_identifier` takes one category segment; the modes category has two.
        let id = mint_identifier(kind, publisher, Some("modes"), slug)?;
        return Ok(match id.rsplit_once(':') {
            Some((head, tail)) => format!("{head}:imported:{tail}"),
            None => id,
        });
    }
    mint_identifier(kind, publisher, category, slug)
}

/// Accumulates the parts and mapping items of one read.
pub(crate) struct Importer {
    publisher: String,
    version: String,
    parts: Vec<ModelPart>,
    items: Vec<MappingItem>,
    used: BTreeSet<String>,
    tool_ids: BTreeMap<String, String>,
}

impl Importer {
    /// `version` is the version every created part carries.
    pub fn new(publisher: &str, version: &str) -> Importer {
        Importer {
            publisher: publisher.to_owned(),
            version: version.to_owned(),
            parts: Vec::new(),
            items: Vec::new(),
            used: BTreeSet::new(),
            tool_ids: BTreeMap::new(),
        }
    }

    /// The publisher must mint: a bad one is refused once, up front.
    pub fn check_publisher(&self) -> Result<(), FormatError> {
        mint_identifier(PartKind::Rule, &self.publisher, None, "probe")
            .map(|_| ())
            .map_err(|e| part_invalid(&self.publisher, &e))
    }

    pub fn item(
        &mut self,
        source_path: &str,
        construct: &str,
        reason: UnmappedReason,
        part_identifier: Option<&str>,
    ) {
        self.items.push(MappingItem {
            source_path: source_path.to_owned(),
            construct: construct.to_owned(),
            reason,
            part_identifier: part_identifier.map(str::to_owned),
        });
    }

    /// Mints a free identifier: a collision appends `-2`, `-3` to the slug in path order.
    pub fn mint(&mut self, kind: PartKind, category: Option<&str>, slug: &str) -> Option<String> {
        for n in 1..=1000 {
            let candidate = if n == 1 {
                slug.to_owned()
            } else {
                format!("{slug}-{n}")
            };
            let id = mint_with(kind, &self.publisher, category, &candidate).ok()?;
            if self.used.insert(id.clone()) {
                return Some(id);
            }
        }
        None
    }

    /// Reads one Markdown file into one part. `Ok(None)` when the file cannot become a part; a
    /// mapping item says why.
    pub fn md_part(&mut self, spec: MdSpec<'_>) -> Result<Option<Created>, FormatError> {
        let MdSpec {
            kind,
            path,
            bytes,
            id,
            default_slug,
            known_keys,
            extras,
            whole_text,
            prompt_inputs,
        } = spec;
        if bytes.len() > MAX_FILE_BYTES {
            self.item(
                path,
                "file larger than 1 MiB",
                UnmappedReason::Unreadable,
                None,
            );
            return Ok(None);
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            self.item(
                path,
                "file not valid UTF-8",
                UnmappedReason::Unreadable,
                None,
            );
            return Ok(None);
        };
        let id = match id {
            IdSpec::Fixed(id) => {
                self.used.insert(id.clone());
                id
            }
            IdSpec::Mint { category, slug } => match self.mint(kind, category, slug) {
                Some(id) => id,
                None => {
                    self.item(path, "file", UnmappedReason::UnrecognisedPath, None);
                    return Ok(None);
                }
            },
        };
        let (front, body) = if whole_text {
            (Front::Absent, text)
        } else {
            front::parse(text)
        };
        let entries = match front {
            Front::Absent => Vec::new(),
            Front::Parsed(entries) => entries,
            Front::Failed(code) => {
                self.item(path, &code, UnmappedReason::Unreadable, Some(&id));
                Vec::new()
            }
        };
        let description = front::text_of(&entries, "description").unwrap_or_default();
        let name = front::text_of(&entries, "name")
            .or_else(|| (!description.is_empty()).then(|| description.clone()))
            .unwrap_or_else(|| default_name(default_slug));
        let mut manifest_body = BTreeMap::new();
        if prompt_inputs {
            self.prompt_inputs(path, &id, &entries, &mut manifest_body);
        }
        for (key, value) in &entries {
            match key.as_str() {
                "name" | "description" => {}
                "argument-hint" | "input" if prompt_inputs => {}
                "applyTo" if kind == PartKind::Rule => {
                    if let Some(glob) = front::scalar(value) {
                        manifest_body
                            .insert("appliesToFiles".to_owned(), YamlValue::Str(glob.to_owned()));
                    }
                }
                other if known_keys.contains(&other) => {}
                "hooks" => self.item(path, "hook", UnmappedReason::Executable, Some(&id)),
                other => self.item(
                    path,
                    &format!("front matter key {other}"),
                    UnmappedReason::UnknownField,
                    Some(&id),
                ),
            }
        }
        let manifest = Manifest {
            api_version: API_VERSION.to_owned(),
            authored_kind: authored(kind),
            metadata: metadata(&id, &name, &description, &self.version),
            body: manifest_body,
        };
        let part = make_part(kind, manifest, Some(body), extras)?;
        self.parts.push(part);
        Ok(Some(Created {
            id,
            name,
            description,
            front: entries,
        }))
    }

    /// FP-TF-2: an APM prompt's `argument-hint` (when a string) and `input` (one short-text input from
    /// its first name; each further name and any other shape a mapping item).
    // spec: ordin-desktop/components/FormatPlugins#FP-TF-2
    fn prompt_inputs(
        &mut self,
        path: &str,
        id: &str,
        entries: &[(String, YamlNode)],
        body: &mut BTreeMap<String, YamlValue>,
    ) {
        let hint = match front::lookup(entries, "argument-hint") {
            Some(YamlNode::Scalar { text, .. }) => {
                body.insert("argument-hint".to_owned(), YamlValue::Str(text.clone()));
                Some(text.clone())
            }
            Some(_) => {
                self.item(
                    path,
                    "front matter key argument-hint",
                    UnmappedReason::UnknownField,
                    Some(id),
                );
                None
            }
            None => None,
        };
        let Some(input) = front::lookup(entries, "input") else {
            return;
        };
        let first = match input {
            YamlNode::Seq(items) => items
                .first()
                .and_then(front::scalar)
                .filter(|name| is_task_input_name(name))
                .map(|name| (name.to_owned(), &items[1..])),
            _ => None,
        };
        let Some((name, rest)) = first else {
            self.item(path, "prompt input", UnmappedReason::UnknownField, Some(id));
            return;
        };
        for item in rest {
            let text = front::scalar(item).unwrap_or("");
            self.item(
                path,
                &format!("prompt input {text}"),
                UnmappedReason::UnknownField,
                Some(id),
            );
        }
        let label = input_label(hint.as_deref(), &name);
        let mut entry = BTreeMap::new();
        entry.insert("name".to_owned(), YamlValue::Str(name));
        entry.insert("label".to_owned(), YamlValue::Str(label));
        entry.insert("kind".to_owned(), YamlValue::Str("short-text".to_owned()));
        body.insert(
            "inputs".to_owned(),
            YamlValue::Seq(vec![YamlValue::Map(entry)]),
        );
    }

    /// A ToolBinding part with one binding; one part per tool name.
    pub fn tool_binding(
        &mut self,
        name: &str,
        binding: YamlValue,
    ) -> Result<Option<String>, FormatError> {
        if let Some(id) = self.tool_ids.get(name) {
            return Ok(Some(id.clone()));
        }
        let Some(id) = self.mint(PartKind::ToolBinding, Some("imported"), name) else {
            return Ok(None);
        };
        let mut body = BTreeMap::new();
        body.insert("bindings".to_owned(), YamlValue::Seq(vec![binding]));
        let manifest = Manifest {
            api_version: API_VERSION.to_owned(),
            authored_kind: authored(PartKind::ToolBinding),
            metadata: metadata(&id, name, "", &self.version),
            body,
        };
        let part = make_part(PartKind::ToolBinding, manifest, None, BTreeMap::new())?;
        self.parts.push(part);
        self.tool_ids.insert(name.to_owned(), id.clone());
        Ok(Some(id))
    }

    /// A WorkspaceProfile referencing `refs`.
    pub fn workspace(
        &mut self,
        id: &str,
        name: &str,
        description: &str,
        refs: &Refs,
    ) -> Result<(), FormatError> {
        let mut body = BTreeMap::new();
        let mut put = |key: &str, ids: &[String]| {
            if !ids.is_empty() {
                body.insert(key.to_owned(), str_seq(ids));
            }
        };
        put("defaultRules", &refs.rules);
        put("skills", &refs.skills);
        put("commands", &refs.commands);
        put("contextResources", &refs.contexts);
        put("allowedOperatingModes", &refs.modes);
        put("toolBindings", &refs.tools);
        if let Some(first) = refs.modes.first() {
            body.insert("defaultMode".to_owned(), YamlValue::Str(first.clone()));
        }
        if let Some(role) = &refs.system_role {
            body.insert("systemRole".to_owned(), YamlValue::Str(role.clone()));
        }
        let mut profile_metadata = metadata(id, name, description, &self.version);
        // The Checker requires a category and an audience role on a workspace profile; an imported
        // one carries the built-in `imported` term for both (FU-097).
        profile_metadata.category = IMPORTED_TERM.to_owned();
        profile_metadata.audience.roles = vec![IMPORTED_TERM.to_owned()];
        let manifest = Manifest {
            api_version: API_VERSION.to_owned(),
            authored_kind: authored(PartKind::WorkspaceProfile),
            metadata: profile_metadata,
            body,
        };
        let part = make_part(PartKind::WorkspaceProfile, manifest, None, BTreeMap::new())?;
        self.parts.push(part);
        Ok(())
    }

    /// A WorkspaceProfile named after the source, referencing every created part
    /// (`source_label` as plain words).
    pub fn source_workspace(&mut self, source_label: &str, refs: &Refs) -> Result<(), FormatError> {
        let id = self
            .mint(PartKind::WorkspaceProfile, None, source_label)
            .or_else(|| self.mint(PartKind::WorkspaceProfile, None, "imported"));
        let Some(id) = id else {
            return Err(FormatError::PartInvalid {
                identifier: self.publisher.clone(),
                code: "AM_ID_INVALID".to_owned(),
            });
        };
        let name = if source_label.trim().is_empty() {
            "Imported agent"
        } else {
            source_label.trim()
        };
        self.workspace(&id, name, "", refs)
    }

    /// One agent file (a Claude Code agent, a Copilot agent): a WorkspaceProfile, its SystemRole
    /// from the file body, and a ToolBinding per name in the front-matter `tools`.
    pub fn agent_profile(
        &mut self,
        path: &str,
        bytes: &[u8],
        slug: &str,
        shared: &Refs,
    ) -> Result<(), FormatError> {
        let probe = std::str::from_utf8(bytes)
            .ok()
            .filter(|_| bytes.len() <= MAX_FILE_BYTES)
            .map(|text| front::parse(text).0);
        let name_hint = match &probe {
            Some(Front::Parsed(entries)) => front::text_of(entries, "name"),
            _ => None,
        }
        .unwrap_or_else(|| slug.to_owned());
        let Some(workspace_id) = self.mint(PartKind::WorkspaceProfile, None, &name_hint) else {
            self.item(path, "agent", UnmappedReason::UnrecognisedPath, None);
            return Ok(());
        };
        let role = self.md_part(MdSpec {
            kind: PartKind::SystemRole,
            path,
            bytes,
            id: IdSpec::Fixed(format!("{workspace_id}:system-role")),
            default_slug: slug,
            known_keys: &["tools"],
            extras: BTreeMap::new(),
            whole_text: false,
            prompt_inputs: false,
        })?;
        let Some(role) = role else {
            self.used.remove(&workspace_id);
            return Ok(());
        };
        let mut own = Refs {
            system_role: Some(role.id.clone()),
            ..Refs::default()
        };
        if let Some(tools) = front::lookup(&role.front, "tools") {
            for tool in front::name_list(tools) {
                let mut binding = BTreeMap::new();
                binding.insert("name".to_owned(), YamlValue::Str(tool.clone()));
                binding.insert("type".to_owned(), YamlValue::Str("builtin".to_owned()));
                if let Some(id) = self.tool_binding(&tool, YamlValue::Map(binding))? {
                    if !own.tools.contains(&id) {
                        own.tools.push(id);
                    }
                }
            }
        }
        let refs = shared.merged(&own);
        self.workspace(&workspace_id, &role.name, &role.description, &refs)
    }

    /// The parts in manifest-path order, and the items in the order they were found.
    pub fn finish(self) -> (Vec<ModelPart>, Vec<MappingItem>) {
        let mut parts = self.parts;
        parts.sort_by(|a, b| a.manifest_path.cmp(&b.manifest_path));
        (parts, self.items)
    }
}

/// FP-TF-2 rule 2: the label of an APM prompt's one input — its `argument-hint` without angle and
/// square brackets, trimmed and cut at 60 characters; else the name with `_` as spaces, capitalised.
// spec: ordin-desktop/components/FormatPlugins#FP-TF-2
pub(crate) fn input_label(hint: Option<&str>, name: &str) -> String {
    let from_hint: String = hint
        .unwrap_or("")
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | '[' | ']'))
        .collect::<String>()
        .trim()
        .chars()
        .take(60)
        .collect();
    let from_hint = from_hint.trim_end().to_owned();
    if !from_hint.is_empty() {
        return from_hint;
    }
    let spaced = name.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
