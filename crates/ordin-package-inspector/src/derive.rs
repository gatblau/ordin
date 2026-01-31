//! Steps 1 to 6 of `inspect` (PackageInspector.md *Internal Logic*): the folder, the source, the
//! parts, the declaration and the authority. Nothing here executes or follows any text it reads.

use std::path::Path;

use ordin_agent_model::{
    canonical_json, content_digest, kind_info, slug_from_id, BundleManifest, LockFile, Manifest,
    MemoryContract, PartKind, PartRef, PartStatus, ResolvedToolPolicy, VocabularySet,
    BUNDLE_MANIFEST_NAME, LOCK_FILE_NAME,
};
use ordin_format_plugins::{
    FormatId, FormatPlugins, MappingItem, ReadRequest, SourceTree, UnmappedReason,
};
use ordin_rules::Declaration;
use ordin_types::domain::agents::Authority;
use ordin_types::time::Timestamp;
use ordin_types::yaml::read_restricted;

use crate::error::InspectError;
use crate::fsio::{
    cut_chars, first_sentence, is_regular_file, primary_manifest, sha256_hex, strip_controls, walk,
    EntryKind, MAX_FOLDER_BYTES,
};
use crate::model::{
    AuthoritySource, ConnectionEntry, InspectRequest, InstallFacts, ModelSource, NotCarried,
    NotCarriedReason, PackageModel, PartLine, PartState, TaskEntry, ToolEntry, ToolKind,
    MODEL_SCHEMA,
};
use crate::places::place_of;

/// The declaration file at the agent folder root (02:478, 08:41).
pub(crate) const DECLARATION_FILE: &str = "wrightrun-declaration.yaml";
/// The APM manifest at the folder root.
const APM_MANIFEST: &str = "apm.yml";
/// 1 MiB, the one declaration limit (Configuration).
pub(crate) const MAX_DECLARATION_BYTES: u64 = 1_048_576;
/// The longest argument hint a task carries (SA-CLOSE-04).
const MAX_HINT_CHARS: usize = 200;

/// A part with the manifest read from the bundle (None when its primary file could not be read).
struct Loaded {
    part: PartRef,
    manifest: Option<Manifest>,
}

/// What steps 1 to 6 produced, with the inputs step 6a needs.
pub(crate) struct Built {
    pub model: PackageModel,
    pub declaration: Option<Declaration>,
    /// Ordin path only (`bundle.manifest.resolved_tool_policy`).
    pub tool_policy: Option<ResolvedToolPolicy>,
    pub package_name: String,
    /// `PartRef.name` of each Command part, in lock order.
    pub commands: Vec<String>,
}

pub(crate) fn derive(
    plugins: &FormatPlugins,
    request: &InspectRequest,
) -> Result<Built, InspectError> {
    // Step 1: the folder.
    let folder = request.folder_path.as_path();
    std::fs::read_dir(folder).map_err(|e| InspectError::folder(&e))?;
    // Step 2: which source.
    let mut built = if is_regular_file(folder, LOCK_FILE_NAME) {
        let bytes =
            std::fs::read(folder.join(LOCK_FILE_NAME)).map_err(|e| InspectError::folder(&e))?;
        let lock = LockFile::from_yaml(&bytes).map_err(|_| InspectError::LockFileInvalid)?;
        ordin_path(request, &lock)?
    } else if is_regular_file(folder, APM_MANIFEST) {
        apm_path(plugins, request)?
    } else {
        return Err(InspectError::FormatUnrecognised);
    };
    // Step 5: the declaration.
    let (declaration, digest) = read_declaration(folder)?;
    built.model.declaration_sha256 = digest;
    built.declaration = declaration;
    // Step 6: the authority.
    // spec: ordin-desktop/components/PackageInspector#SA-CLOSE-01
    built.model.authority = authority_of(&built);
    built.model.authority_source = AuthoritySource::Derived;
    Ok(built)
}

/// Step 6: `system_changing` when any tool has a permission other than `read-only`, or any connection
/// exists, or the tool policy's `requires_human_approval` is non-empty, or the declaration's
/// `approval_required` is non-empty; else `document_only`. Never `routing_only`.
fn authority_of(built: &Built) -> Authority {
    let acting_tool = built
        .model
        .tools
        .iter()
        .any(|tool| tool.permission != "read-only");
    let connected = !built.model.connections.is_empty();
    let policy_asks = built
        .tool_policy
        .as_ref()
        .is_some_and(|policy| !policy.requires_human_approval.is_empty());
    let declaration_asks = built
        .declaration
        .as_ref()
        .is_some_and(|declaration| !declaration.approval_required.is_empty());
    if acting_tool || connected || policy_asks || declaration_asks {
        Authority::SystemChanging
    } else {
        Authority::DocumentOnly
    }
}

/// Step 5: the declaration file, when present.
fn read_declaration(folder: &Path) -> Result<(Option<Declaration>, Option<String>), InspectError> {
    if !is_regular_file(folder, DECLARATION_FILE) {
        return Ok((None, None));
    }
    let path = folder.join(DECLARATION_FILE);
    let size = std::fs::metadata(&path)
        .map_err(|e| InspectError::folder(&e))?
        .len();
    if size > MAX_DECLARATION_BYTES {
        return Err(InspectError::DeclarationTooLarge { bytes: size });
    }
    let bytes = std::fs::read(&path).map_err(|e| InspectError::folder(&e))?;
    if bytes.len() as u64 > MAX_DECLARATION_BYTES {
        return Err(InspectError::DeclarationTooLarge {
            bytes: bytes.len() as u64,
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| InspectError::DeclarationInvalid)?;
    let declaration = read_restricted::<Declaration>(text, MAX_DECLARATION_BYTES as usize)
        .map_err(|_| InspectError::DeclarationInvalid)?;
    let digest = sha256_hex(&bytes);
    tracing::info!(
        declaration_sha256 = digest.as_str(),
        "package_inspector.declaration_loaded"
    );
    Ok((Some(declaration), Some(digest)))
}

fn hex_of(prefixed: &str) -> &str {
    prefixed.strip_prefix("sha256:").unwrap_or(prefixed)
}

/// Step 3: the ordin path.
fn ordin_path(request: &InspectRequest, lock: &LockFile) -> Result<Built, InspectError> {
    let bundle_dir = request
        .bundle_dir
        .as_deref()
        .ok_or(InspectError::BundleUnreadable)?;
    let manifest_bytes = std::fs::read(bundle_dir.join(BUNDLE_MANIFEST_NAME))
        .map_err(|_| InspectError::BundleUnreadable)?;
    let bundle: BundleManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|_| InspectError::BundleUnreadable)?;
    if bundle.plan_hash != lock.plan_hash {
        return Err(InspectError::BundleUnreadable);
    }
    let read_part = |part: &PartRef| -> Option<Manifest> {
        let dir = bundle_dir
            .join("artefacts")
            .join(hex_of(&part.content_hash));
        primary_manifest(&dir).map(|(_, _, manifest)| manifest)
    };
    let loaded: Vec<Loaded> = lock
        .artefacts
        .iter()
        .map(|part| Loaded {
            part: part.clone(),
            manifest: read_part(part),
        })
        .collect();
    let composition = read_part(&lock.composition);
    let install = request.install.clone().unwrap_or_default();
    let policy = bundle.resolved_tool_policy.clone();

    let mut parts = Derivation::new(request, Some(&lock.composition.repository));
    parts.add_all(&loaded, Some(&install));

    // Connections come from the resolved tool policy.
    let connections = policy
        .mcp_servers
        .iter()
        .map(|server| ConnectionEntry {
            name: server.name.clone(),
            server_type: server.server_type.clone(),
            transport: server.transport.clone(),
            needs_credential: server.env_ref.is_some(),
        })
        .collect();
    // Not carried: kinds not offered, then the Installers' own reference-only list.
    for line in &loaded {
        if !kind_info(line.part.kind).offered {
            parts.not_carried.push(NotCarried {
                what: line.part.id.clone(),
                reason: NotCarriedReason::KindNotOffered,
            });
        }
    }
    for id in &install.reference_only_parts {
        parts.not_carried.push(NotCarried {
            what: id.clone(),
            reason: NotCarriedReason::ReferenceOnly,
        });
    }
    let policy_digest = serde_json::to_value(&policy)
        .map(|value| sha256_hex(&canonical_json(&value)))
        .map_err(|_| InspectError::BundleUnreadable)?;
    let (display_name, description) = match &composition {
        Some(manifest) => (
            display_name_of(manifest),
            first_sentence(&manifest.metadata.description),
        ),
        None => (lock.composition.name.clone(), String::new()),
    };
    let commands = parts.commands.clone();
    let model = PackageModel {
        model_schema: MODEL_SCHEMA.to_owned(),
        agent_instance_id: request.agent_instance_id,
        source: ModelSource::OrdinBundle {
            plan_hash: lock.plan_hash.clone(),
            repository: lock.composition.repository.clone(),
            commit: lock.composition.commit.clone(),
        },
        display_name,
        description,
        tasks: parts.tasks,
        tools: parts.tools,
        connections,
        authority: Authority::DocumentOnly,
        authority_source: AuthoritySource::Derived,
        not_carried: parts.not_carried,
        declaration_sha256: None,
        declared_rules: Vec::new(),
        tool_policy_sha256: Some(policy_digest),
        plugin_name: install.plugin_name.clone(),
        needs_person_present: install.needs_person_present,
        memory: parts.memory,
        parts: parts.lines,
        inspected_at: Timestamp::now(),
    };
    Ok(Built {
        model,
        declaration: None,
        tool_policy: Some(policy),
        package_name: lock.composition.name.clone(),
        commands,
    })
}

/// Step 4: the APM-era path (R-73, ADR-093 item 6).
fn apm_path(plugins: &FormatPlugins, request: &InspectRequest) -> Result<Built, InspectError> {
    let files = walk(&request.folder_path, MAX_FOLDER_BYTES)?;
    let mut tree = std::collections::BTreeMap::new();
    for file in files.iter().filter(|file| file.kind == EntryKind::File) {
        let bytes = std::fs::read(&file.path).map_err(|e| InspectError::folder(&e))?;
        tree.insert(file.rel.clone(), bytes);
    }
    let read = ReadRequest {
        publisher: "imported".to_owned(),
        vocabulary: VocabularySet::default(),
        source_label: "an installed APM package".to_owned(),
    };
    let outcome = plugins
        .get(FormatId::Apm)
        .read(&SourceTree(tree), &read)
        .map_err(|_| InspectError::FormatUnrecognised)?;
    let package = outcome.package.ok_or(InspectError::FormatUnrecognised)?;
    let mut description = String::new();
    let mut loaded = Vec::new();
    for part in &outcome.parts {
        if part.kind == PartKind::WorkspaceProfile {
            description = first_sentence(&part.manifest.metadata.description);
            continue;
        }
        loaded.push(Loaded {
            part: PartRef {
                kind: part.kind,
                id: part.manifest.metadata.id.clone(),
                name: slug_from_id(&part.manifest.metadata.id),
                version: part.manifest.metadata.version.clone(),
                content_hash: format!("sha256:{}", content_digest(&part.files).0),
                repository: String::new(),
                commit: String::new(),
                loading: None,
            },
            manifest: Some(part.manifest.clone()),
        });
    }
    let mut parts = Derivation::new(request, None);
    parts.add_all(&loaded, None);
    for item in &outcome.mapping_report.items {
        parts.not_carried.push(not_carried_of(item));
    }
    let commands = parts.commands.clone();
    let model = PackageModel {
        model_schema: MODEL_SCHEMA.to_owned(),
        agent_instance_id: request.agent_instance_id,
        source: ModelSource::ApmFolder {
            package_name: package.name.clone(),
            package_version: package.version.clone(),
        },
        display_name: package.name.clone(),
        description,
        tasks: parts.tasks,
        tools: parts.tools,
        connections: Vec::new(),
        authority: Authority::DocumentOnly,
        authority_source: AuthoritySource::Derived,
        not_carried: parts.not_carried,
        declaration_sha256: None,
        declared_rules: Vec::new(),
        tool_policy_sha256: None,
        plugin_name: None,
        needs_person_present: false,
        memory: None,
        parts: parts.lines,
        inspected_at: Timestamp::now(),
    };
    Ok(Built {
        model,
        declaration: None,
        tool_policy: None,
        package_name: package.name,
        commands,
    })
}

/// Every mapping item becomes a `NotCarried`, its reason mapped one to one.
fn not_carried_of(item: &MappingItem) -> NotCarried {
    // `UnmappedReason::UnrecognisedPath` has no `NotCarriedReason` of the same meaning; it is
    // carried as `NoEquivalentKind`, the nearest (a file the format has no place for).
    let reason = match item.reason {
        UnmappedReason::NoEquivalentKind | UnmappedReason::UnrecognisedPath => {
            NotCarriedReason::NoEquivalentKind
        }
        UnmappedReason::UnknownField => NotCarriedReason::UnknownField,
        UnmappedReason::Executable => NotCarriedReason::Executable,
        UnmappedReason::Unreadable => NotCarriedReason::Unreadable,
    };
    NotCarried {
        what: item.source_path.clone(),
        reason,
    }
}

fn display_name_of(manifest: &Manifest) -> String {
    if manifest.metadata.name.is_empty() {
        manifest.slug()
    } else {
        manifest.metadata.name.clone()
    }
}

/// The lines, tasks, tools and memory shared by both paths.
struct Derivation<'a> {
    request: &'a InspectRequest,
    composition_repository: Option<&'a str>,
    lines: Vec<PartLine>,
    tasks: Vec<TaskEntry>,
    tools: Vec<ToolEntry>,
    memory: Option<MemoryContract>,
    not_carried: Vec<NotCarried>,
    commands: Vec<String>,
}

impl<'a> Derivation<'a> {
    fn new(request: &'a InspectRequest, composition_repository: Option<&'a str>) -> Self {
        Derivation {
            request,
            composition_repository,
            lines: Vec::new(),
            tasks: Vec::new(),
            tools: Vec::new(),
            memory: None,
            not_carried: Vec::new(),
            commands: Vec::new(),
        }
    }

    fn add_all(&mut self, loaded: &[Loaded], install: Option<&InstallFacts>) {
        let mut memory_seen = false;
        for item in loaded {
            let Some(manifest) = &item.manifest else {
                self.not_carried.push(NotCarried {
                    what: item.part.id.clone(),
                    reason: NotCarriedReason::Unreadable,
                });
                continue;
            };
            let line = self.line_of(&item.part, manifest);
            match item.part.kind {
                PartKind::Command => {
                    self.commands.push(item.part.name.clone());
                    self.tasks
                        .push(self.task_of(&item.part, manifest, &line, install));
                }
                PartKind::ToolBinding => self.add_tools(&item.part, manifest),
                PartKind::Memory if !memory_seen => {
                    memory_seen = true;
                    match manifest.memory_contract() {
                        Ok(contract) => self.memory = contract,
                        Err(_) => self.not_carried.push(NotCarried {
                            what: item.part.id.clone(),
                            reason: NotCarriedReason::Unreadable,
                        }),
                    }
                }
                _ => {}
            }
            self.lines.push(line);
        }
        self.tools.sort_by(|a, b| {
            a.name
                .as_bytes()
                .cmp(b.name.as_bytes())
                .then_with(|| a.declared_by.cmp(&b.declared_by))
        });
    }

    fn line_of(&self, part: &PartRef, manifest: &Manifest) -> PartLine {
        let state = match self.request.part_states.get(&part.id) {
            Some(state) => *state,
            None => match manifest.status_or_default() {
                Ok(PartStatus::Deprecated) => PartState::NotForNewAgents,
                _ => PartState::InUse,
            },
        };
        let from = match self.composition_repository {
            Some(own) if own != part.repository => {
                self.request.repository_names.get(&part.repository).cloned()
            }
            _ => None,
        };
        PartLine {
            part: part.clone(),
            name: display_name_of(manifest),
            what_it_does: first_sentence(&manifest.metadata.description),
            state,
            from,
        }
    }

    fn task_of(
        &self,
        part: &PartRef,
        manifest: &Manifest,
        line: &PartLine,
        install: Option<&InstallFacts>,
    ) -> TaskEntry {
        let argument_hint = match manifest.argument_hint() {
            Ok(hint) => hint.map(|text| cut_chars(&strip_controls(&text), MAX_HINT_CHARS)),
            Err(_) => {
                tracing::warn!(
                    agent_instance_id = self.request.agent_instance_id.to_string().as_str(),
                    command_id = part.id.as_str(),
                    "package_inspector.argument_hint_invalid"
                );
                None
            }
        };
        // PKI-TF-1: the title and inputs; either not reading leaves both None and the form
        // unreadable, logged with the error code only.
        // spec: ordin-desktop/components/PackageInspector#PKI-TF-1
        let (title, inputs, form_unreadable) = match (manifest.title(), manifest.inputs()) {
            (Ok(title), Ok(inputs)) => (title, inputs, false),
            (Err(error), _) | (_, Err(error)) => {
                tracing::warn!(
                    agent_instance_id = self.request.agent_instance_id.to_string().as_str(),
                    command_id = part.id.as_str(),
                    code = error.code(),
                    "package_inspector.task_form_invalid"
                );
                (None, None, true)
            }
        };
        TaskEntry {
            command_id: part.id.clone(),
            name: line.name.clone(),
            description: line.what_it_does.clone(),
            session_name: install.and_then(|facts| facts.command_names.get(&part.id).cloned()),
            argument_hint,
            title,
            inputs,
            form_unreadable,
        }
    }

    fn add_tools(&mut self, part: &PartRef, manifest: &Manifest) {
        let Ok(bindings) = manifest.tool_bindings() else {
            // Carried as `Unreadable`, as a Memory part with a bad contract is.
            self.not_carried.push(NotCarried {
                what: part.id.clone(),
                reason: NotCarriedReason::Unreadable,
            });
            return;
        };
        for binding in bindings {
            let mut arguments = Vec::new();
            for pointer in &binding.argument_paths {
                match place_of(pointer) {
                    Some(place) => arguments.push(place),
                    None => tracing::warn!(
                        agent_instance_id = self.request.agent_instance_id.to_string().as_str(),
                        tool = binding.name.as_str(),
                        "package_inspector.argument_place_skipped"
                    ),
                }
            }
            self.tools.push(ToolEntry {
                kind: if binding.binding_type == "mcp" {
                    ToolKind::McpServer
                } else {
                    ToolKind::BuiltIn
                },
                name: binding.name,
                permission: binding.permission,
                declared_by: part.id.clone(),
                arguments,
            });
        }
    }
}
