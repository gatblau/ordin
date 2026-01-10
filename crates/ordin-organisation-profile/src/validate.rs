//! The bounds of a parsed document, in the order of the *Validation table* (Internal Logic 3.6, with the
//! rows the ADR-102, ADR-110 and ADR-113 amendments add). The first broken rule is reported as
//! `PROF_CONTENT_INVALID` with its dotted key path.

use std::collections::HashSet;

use ordin_types::text::{check_https_url, AreaCodes};

use crate::document::*;
use crate::error::ProfileError;

const CONTENT: &str = "PROF_CONTENT_INVALID";
const CODES: AreaCodes = AreaCodes {
    name_empty: CONTENT,
    name_too_long: CONTENT,
    path_invalid: CONTENT,
    url_invalid: CONTENT,
};

/// SA-GH-01's write kinds (`engineering.github.always_approved_kinds`).
const GITHUB_WRITE_KINDS: [&str; 11] = [
    "comment",
    "open_pull_request",
    "update_pull_request",
    "request_review",
    "push_branch",
    "create_branch",
    "delete_branch",
    "label",
    "merge",
    "release",
    "close",
];

type Checked = Result<(), ProfileError>;

/// Applies every rule in table order.
pub(crate) fn validate(doc: &OrganisationProfileDocument) -> Checked {
    profile_version(doc)?;
    repositories(doc)?;
    team_knowledge(doc)?;
    knowledge_agents(doc)?;
    trust_policy(doc)?;
    destinations(doc)?;
    remote_agents(doc)?;
    slack(doc)?;
    confluence(doc)?;
    microsoft(doc)?;
    google(doc)?;
    identity_provider(doc)?;
    harnesses(doc)?;
    usage(doc)?;
    voice(doc)?;
    limits(doc)?;
    history(doc)?;
    email(doc)?;
    channels(doc)?;
    telegram(doc)?;
    server(doc)?;
    nats(doc)?;
    craft(doc)?;
    gitlab(doc)?;
    language_servers(doc)?;
    engineering(doc)?;
    workflows(doc)?;
    brand(doc)?;
    locales(doc)?;
    updates(doc)?;
    sandbox(doc)
}

// ---- rule helpers

/// Refuses at `path` unless `ok`.
fn ensure(ok: bool, path: impl FnOnce() -> String) -> Checked {
    if ok {
        Ok(())
    } else {
        Err(ProfileError::content(path()))
    }
}

/// `min..=max` Unicode scalar values.
fn chars_between(value: &str, min: usize, max: usize) -> bool {
    let count = value.chars().count();
    (min..=max).contains(&count)
}

/// `^\d{1,9}\.\d{1,9}\.\d{1,9}$`: three numeric parts, each 1–9 digits.
pub(crate) fn is_version(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| (1..=9).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The numeric parts of a value `is_version` accepts.
pub(crate) fn version_parts(value: &str) -> Option<[u64; 3]> {
    if !is_version(value) {
        return None;
    }
    let mut parts = [0u64; 3];
    for (slot, part) in parts.iter_mut().zip(value.split('.')) {
        *slot = part.parse().ok()?;
    }
    Some(parts)
}

/// `^[a-z0-9-]{1,64}$`.
fn is_key(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A DNS name of at most `max` bytes: dot-separated labels of 1–63 letters, digits and hyphens,
/// neither starting nor ending with a hyphen. No scheme, path or port can pass.
fn is_dns_name(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

/// `owner/name`, each part `^[A-Za-z0-9_.-]{1,100}$`.
fn is_repository(value: &str) -> bool {
    let part = |p: &str| {
        (1..=100).contains(&p.len())
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-')
    };
    match value.split_once('/') {
        Some((owner, name)) => part(owner) && part(name),
        None => false,
    }
}

/// An absolute `https` URL of at most 2,048 bytes.
fn is_https(value: &str) -> bool {
    check_https_url(value, false, CODES).is_ok()
}

/// A canonical-form UUID (8-4-4-4-12 hexadecimal digits).
fn is_uuid(value: &str) -> bool {
    let groups: Vec<&str> = value.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8usize, 4, 4, 4, 12])
            .all(|(g, len)| g.len() == len && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A PEM certificate of at most 65,536 bytes, starting `-----BEGIN CERTIFICATE-----`.
fn is_pem(value: &str) -> bool {
    value.len() <= 65_536 && value.starts_with("-----BEGIN CERTIFICATE-----")
}

/// A BCP 47 tag in the profile's form: `^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$`, at most 35 characters.
fn is_locale_tag(value: &str) -> bool {
    if value.len() > 35 {
        return false;
    }
    let mut parts = value.split('-');
    let first_ok = parts
        .next()
        .is_some_and(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphabetic()));
    first_ok
        && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// A Slack channel id, `^[CG][A-Z0-9]{8,20}$` (SA-CLOSE-04).
// spec: ordin-desktop/components/OrganisationProfile#SA-CLOSE-04
pub(crate) fn is_slack_channel_id(value: &str) -> bool {
    slack_id(value, b"CG")
}

/// A Slack member id, `^[UW][A-Z0-9]{8,20}$` (SA-CLOSE-04).
pub(crate) fn is_slack_member_id(value: &str) -> bool {
    slack_id(value, b"UW")
}

fn slack_id(value: &str, first: &[u8]) -> bool {
    let bytes = value.as_bytes();
    (9..=21).contains(&bytes.len())
        && first.contains(&bytes[0])
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// A file name of 1–255 characters with no `/`, `\` or `..`.
fn is_file_name(value: &str) -> bool {
    chars_between(value, 1, 255)
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains("..")
}

/// No control character.
fn no_control(value: &str) -> bool {
    !value.chars().any(char::is_control)
}

/// True when every item is distinct.
fn distinct<T: PartialEq>(items: &[T]) -> Result<(), usize> {
    for (i, item) in items.iter().enumerate() {
        if items[..i].contains(item) {
            return Err(i);
        }
    }
    Ok(())
}

/// A valid git ref name of 1–255 bytes (the rules of `git check-ref-format`, one-level names allowed).
fn is_git_ref(value: &str) -> bool {
    if value.is_empty() || value.len() > 255 || value == "@" {
        return false;
    }
    if value.starts_with('/') || value.ends_with('/') || value.ends_with('.') {
        return false;
    }
    if value.contains("..") || value.contains("//") || value.contains("@{") {
        return false;
    }
    if value.chars().any(|c| {
        c.is_ascii_control() || matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    }) {
        return false;
    }
    value
        .split('/')
        .all(|part| !part.starts_with('.') && !part.ends_with(".lock"))
}

// ---- the table, row by row

fn profile_version(doc: &OrganisationProfileDocument) -> Checked {
    ensure(is_version(&doc.profile_version), || {
        "profile_version".into()
    })
}

fn repositories(doc: &OrganisationProfileDocument) -> Checked {
    let repos = &doc.agent_catalogue.repositories;
    let base = "agent_catalogue.repositories";
    ensure(repos.len() <= 50, || base.into())?;
    let mut keys = HashSet::new();
    let mut names = HashSet::new();
    for (i, r) in repos.iter().enumerate() {
        let at = |field: &str| format!("{base}[{i}].{field}");
        ensure(is_key(&r.key) && keys.insert(r.key.as_str()), || at("key"))?;
        ensure(
            chars_between(&r.display_name, 1, 120) && names.insert(r.display_name.as_str()),
            || at("display_name"),
        )?;
        ensure(is_dns_name(&r.host, 253), || at("host"))?;
        ensure(is_repository(&r.repository), || at("repository"))?;
        ensure(chars_between(&r.protected_branch, 1, 255), || {
            at("protected_branch")
        })?;
    }
    Ok(())
}

/// Amendment January 2026 (ADR-110): `team_knowledge.read_folders`.
fn team_knowledge(doc: &OrganisationProfileDocument) -> Checked {
    let folders = &doc.team_knowledge.read_folders;
    let base = "team_knowledge.read_folders";
    // spec: ordin-desktop/components/OrganisationProfile#SA-PROFR-01
    ensure(folders.len() <= 20, || base.into())?;
    // spec: ordin-desktop/components/OrganisationProfile#SA-PROFR-02
    let gitlab_host = doc
        .gitlab
        .as_ref()
        .map_or("gitlab.com", |g| g.host.as_str());
    let mut keys = HashSet::new();
    let mut names = HashSet::new();
    let mut places = HashSet::new();
    for (i, f) in folders.iter().enumerate() {
        let at = |field: &str| format!("{base}[{i}].{field}");
        ensure(is_key(&f.key) && keys.insert(f.key.as_str()), || at("key"))?;
        ensure(
            chars_between(&f.display_name, 1, 120)
                && no_control(&f.display_name)
                && names.insert(f.display_name.as_str()),
            || at("display_name"),
        )?;
        ensure(f.host == "github.com" || f.host == gitlab_host, || {
            at("host")
        })?;
        ensure(is_repository(&f.repository), || at("repository"))?;
        ensure(is_git_ref(&f.branch), || at("branch"))?;
        ensure(is_relative_folder(&f.folder), || at("folder"))?;
        ensure(
            places.insert((&f.host, &f.repository, &f.branch, &f.folder)),
            || format!("{base}[{i}]"),
        )?;
    }
    Ok(())
}

/// `folder` ≤ 1,024 bytes, relative, no segment `..` or `.`, no leading or trailing `/`; `""` is the root.
fn is_relative_folder(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    value.len() <= 1024
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains('\\')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// Amendment January 2026 (ADR-113): `knowledge_agents.front_doors`.
fn knowledge_agents(doc: &OrganisationProfileDocument) -> Checked {
    let doors = &doc.knowledge_agents.front_doors;
    let base = "knowledge_agents.front_doors";
    // spec: ordin-desktop/components/OrganisationProfile#SA-PROF-KA-01
    ensure(doors.len() <= 8, || base.into())?;
    let mut keys = HashSet::new();
    let mut names = HashSet::new();
    let mut urls = HashSet::new();
    for (i, d) in doors.iter().enumerate() {
        let at = |field: &str| format!("{base}[{i}].{field}");
        ensure(is_key(&d.key) && keys.insert(d.key.as_str()), || at("key"))?;
        ensure(
            chars_between(&d.display_name, 1, 120)
                && no_control(&d.display_name)
                && names.insert(d.display_name.as_str()),
            || at("display_name"),
        )?;
        ensure(
            is_front_door_url(&d.url) && urls.insert(front_door_key(&d.url)),
            || at("url"),
        )?;
    }
    Ok(())
}

/// SA-PROF-KA-02: `https` only, no user information, query or fragment, at most 2,048 bytes, no
/// trailing `/`.
// spec: ordin-desktop/components/OrganisationProfile#SA-PROF-KA-02
fn is_front_door_url(value: &str) -> bool {
    match check_https_url(value, false, CODES) {
        Ok(url) => {
            url.scheme() == "https"
                && url.query().is_none()
                && url.fragment().is_none()
                && !value.contains('?')
                && !value.contains('#')
                && !value.ends_with('/')
        }
        Err(_) => false,
    }
}

/// The address with its scheme and host lower-cased, for the uniqueness rule.
fn front_door_key(value: &str) -> String {
    let after_scheme = value.find("://").map_or(0, |i| i + 3);
    let authority_end = value[after_scheme..]
        .find('/')
        .map_or(value.len(), |i| after_scheme + i);
    format!(
        "{}{}",
        value[..authority_end].to_ascii_lowercase(),
        &value[authority_end..]
    )
}

fn trust_policy(doc: &OrganisationProfileDocument) -> Checked {
    let policy = &doc.agent_catalogue.trust_policy;
    let size = serde_json::to_vec(policy).map_or(usize::MAX, |b| b.len());
    ensure(policy.is_object() && size <= 262_144, || {
        "agent_catalogue.trust_policy".into()
    })
}

fn destinations(doc: &OrganisationProfileDocument) -> Checked {
    if let Some(d) = &doc.agent_catalogue.it_approval_destination {
        ensure(is_destination(d), || {
            "agent_catalogue.it_approval_destination.address".into()
        })?;
    }
    if let Some(d) = &doc.help.destination {
        ensure(is_destination(d), || "help.destination.address".into())?;
    }
    Ok(())
}

fn is_destination(d: &HelpDestination) -> bool {
    let a = d.address.as_str();
    match d.kind {
        HelpDestinationKind::TicketUrl => is_https(a),
        HelpDestinationKind::Email => {
            chars_between(a, 1, 254)
                && a.matches('@').count() == 1
                && a.split_once('@')
                    .is_some_and(|(l, r)| !l.is_empty() && !r.is_empty())
        }
        HelpDestinationKind::SlackChannel => is_slack_channel_name(a) || is_help_channel_id(a),
    }
}

/// `^#?[a-z0-9_-]{1,80}$`.
fn is_slack_channel_name(value: &str) -> bool {
    let name = value.strip_prefix('#').unwrap_or(value);
    (1..=80).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// `^C[A-Z0-9]{8,12}$`.
fn is_help_channel_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    (9..=13).contains(&bytes.len())
        && bytes[0] == b'C'
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn remote_agents(doc: &OrganisationProfileDocument) -> Checked {
    let agents = &doc.remote_agents;
    let base = "remote_agents";
    ensure(agents.len() <= 100, || base.into())?;
    let mut names = HashSet::new();
    for (i, a) in agents.iter().enumerate() {
        let at = |field: &str| format!("{base}[{i}].{field}");
        ensure(
            chars_between(&a.name, 1, 120) && names.insert(a.name.as_str()),
            || at("name"),
        )?;
        match a.route {
            RemoteAgentRoute::HostedAgent => {
                ensure(is_https(&a.address), || at("address"))?;
                let Some(identity) = &a.identity else {
                    return Err(ProfileError::content(at("identity")));
                };
                ensure(is_https(&identity.issuer), || at("identity.issuer"))?;
                ensure(chars_between(&identity.client_id, 1, 255), || {
                    at("identity.client_id")
                })?;
                ensure(chars_between(&identity.audience, 1, 255), || {
                    at("identity.audience")
                })?;
            }
            RemoteAgentRoute::Slack => {
                ensure(is_slack_member_id(&a.address), || at("address"))?;
                ensure(a.identity.is_none(), || at("identity"))?;
                ensure(doc.slack.is_some(), || "slack".into())?;
                // SPEC-GAP-4: ADR-102 made `slack.dedicated_channel` optional, but a Slack remote agent
                // becomes a `remote_agents` row whose `slack_channel` is required (2C RemoteAgent rule,
                // RAG_SLACK_FIELDS_INVALID; the table's CHECK). A Slack entry without the dedicated
                // channel is therefore refused here, before activation, rather than failing the
                // activation transaction.
                ensure(
                    doc.slack
                        .as_ref()
                        .is_some_and(|s| s.dedicated_channel.is_some()),
                    || "slack.dedicated_channel".into(),
                )?;
            }
        }
    }
    Ok(())
}

fn slack(doc: &OrganisationProfileDocument) -> Checked {
    let Some(s) = &doc.slack else {
        return Ok(());
    };
    if let Some(workspace) = &s.workspace {
        ensure(chars_between(workspace, 1, 255), || {
            "slack.workspace".into()
        })?;
    }
    if let Some(channel) = &s.dedicated_channel {
        ensure(is_slack_channel_id(channel), || {
            "slack.dedicated_channel".into()
        })?;
    }
    match (s.workspace.is_some(), s.dedicated_channel.is_some()) {
        (true, false) => return Err(ProfileError::content("slack.dedicated_channel")),
        (false, true) => return Err(ProfileError::content("slack.workspace")),
        _ => {}
    }
    if let Some(client_id) = &s.client_id {
        ensure(chars_between(client_id, 1, 255), || {
            "slack.client_id".into()
        })?;
    }
    if let Some(channels) = &s.allowed_channels {
        ensure(channels.len() <= 200, || "slack.allowed_channels".into())?;
        for (i, c) in channels.iter().enumerate() {
            ensure(is_slack_channel_id(c) && !channels[..i].contains(c), || {
                format!("slack.allowed_channels[{i}]")
            })?;
        }
    }
    Ok(())
}

/// `confluence.token_exchange_url` is carried as the fallback of C-12: absent → public client with PKCE.
fn confluence(doc: &OrganisationProfileDocument) -> Checked {
    let Some(c) = &doc.confluence else {
        return Ok(());
    };
    ensure(is_https(&c.site), || "confluence.site".into())?;
    ensure(chars_between(&c.oauth_client_id, 1, 255), || {
        "confluence.oauth_client_id".into()
    })?;
    if let Some(url) = &c.token_exchange_url {
        ensure(is_https(url), || "confluence.token_exchange_url".into())?;
    }
    Ok(())
}

/// The Microsoft public-client settings of C-14.
fn microsoft(doc: &OrganisationProfileDocument) -> Checked {
    let Some(m) = &doc.microsoft else {
        return Ok(());
    };
    ensure(
        is_uuid(&m.tenant_id) || is_dns_name(&m.tenant_id, 253),
        || "microsoft.tenant_id".into(),
    )?;
    ensure(is_uuid(&m.client_id), || "microsoft.client_id".into())
}

fn google(doc: &OrganisationProfileDocument) -> Checked {
    let Some(g) = &doc.google else {
        return Ok(());
    };
    ensure(chars_between(&g.client_id, 1, 255), || {
        "google.client_id".into()
    })?;
    if let Some(secret) = &g.client_secret {
        ensure(chars_between(secret, 1, 255), || {
            "google.client_secret".into()
        })?;
    }
    Ok(())
}

fn identity_provider(doc: &OrganisationProfileDocument) -> Checked {
    let Some(p) = &doc.identity_provider else {
        return Ok(());
    };
    ensure(is_https(&p.issuer), || "identity_provider.issuer".into())?;
    ensure(chars_between(&p.client_id, 1, 255), || {
        "identity_provider.client_id".into()
    })?;
    ensure(p.scopes.len() <= 20, || "identity_provider.scopes".into())?;
    for (i, scope) in p.scopes.iter().enumerate() {
        ensure(
            (1..=128).contains(&scope.len()) && scope.bytes().all(|b| (0x21..=0x7e).contains(&b)),
            || format!("identity_provider.scopes[{i}]"),
        )?;
    }
    Ok(())
}

fn harnesses(doc: &OrganisationProfileDocument) -> Checked {
    let list = &doc.harnesses;
    ensure((1..=2).contains(&list.len()), || "harnesses".into())?;
    for (i, h) in list.iter().enumerate() {
        ensure(!list[..i].iter().any(|o| o.name == h.name), || {
            format!("harnesses[{i}].name")
        })?;
        ensure(h.tested_versions.len() <= 50, || {
            format!("harnesses[{i}].tested_versions")
        })?;
        for (j, v) in h.tested_versions.iter().enumerate() {
            ensure(chars_between(v, 1, 64), || {
                format!("harnesses[{i}].tested_versions[{j}]")
            })?;
        }
    }
    Ok(())
}

fn usage(doc: &OrganisationProfileDocument) -> Checked {
    let u = &doc.usage;
    for (kind, levels) in &u.levels_allowed {
        let at = || format!("usage.levels_allowed.{}", serde_name(kind));
        ensure((1..=4).contains(&levels.len()), at)?;
        ensure(distinct(levels).is_ok(), at)?;
    }
    for (harness, levels) in &u.level_models {
        for (level, model) in levels {
            ensure(is_model_name(model), || {
                format!(
                    "usage.level_models.{}.{}",
                    serde_name(harness),
                    serde_name(level)
                )
            })?;
        }
    }
    ensure((1..=3).contains(&u.agents_at_once), || {
        "usage.agents_at_once".into()
    })?;
    ensure((1..=3).contains(&u.agents_at_once_max), || {
        "usage.agents_at_once_max".into()
    })?;
    ensure(u.agents_at_once <= u.agents_at_once_max, || {
        "usage.agents_at_once".into()
    })?;
    if let Some(names) = &u.person_may_change {
        if let Err(i) = distinct(names) {
            return Err(ProfileError::content(format!(
                "usage.person_may_change[{i}]"
            )));
        }
    }
    Ok(())
}

/// The adapters' `accepts_model` (C-3): 1–100 characters of `[A-Za-z0-9._-]`.
fn is_model_name(value: &str) -> bool {
    (1..=100).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// The snake_case name serde writes for a unit variant (used in key paths of map keys).
fn serde_name<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

fn voice(doc: &OrganisationProfileDocument) -> Checked {
    for (name, terms) in [
        ("voice.terms_use", &doc.voice.terms_use),
        ("voice.terms_avoid", &doc.voice.terms_avoid),
    ] {
        ensure(terms.len() <= 200, || name.into())?;
        for (i, t) in terms.iter().enumerate() {
            ensure(chars_between(t, 1, 80), || format!("{name}[{i}]"))?;
        }
    }
    Ok(())
}

fn limits(doc: &OrganisationProfileDocument) -> Checked {
    let crew = &doc.limits.crew;
    ensure((1..=30).contains(&crew.rounds), || {
        "limits.crew.rounds".into()
    })?;
    ensure((1..=480).contains(&crew.wall_clock_minutes), || {
        "limits.crew.wall_clock_minutes".into()
    })?;
    ensure((1..=8).contains(&crew.members_at_once), || {
        "limits.crew.members_at_once".into()
    })?;
    ensure(crew.request_budget.is_none_or(|b| b >= 1), || {
        "limits.crew.request_budget".into()
    })?;
    ensure(
        (1..=480).contains(&doc.limits.run.wall_clock_minutes),
        || "limits.run.wall_clock_minutes".into(),
    )
}

fn history(doc: &OrganisationProfileDocument) -> Checked {
    ensure(doc.history.max_days.is_none_or(|d| d >= 1), || {
        "history.max_days".into()
    })
}

fn email(doc: &OrganisationProfileDocument) -> Checked {
    let Some(e) = &doc.email else {
        return Ok(());
    };
    if e.system == MailSystem::CompanyServer {
        ensure(e.imap.is_some(), || "email.imap".into())?;
        ensure(e.smtp.is_some(), || "email.smtp".into())?;
        ensure(!e.company_domains.is_empty(), || {
            "email.company_domains".into()
        })?;
    }
    for (name, server) in [("email.imap", &e.imap), ("email.smtp", &e.smtp)] {
        if let Some(s) = server {
            ensure(is_dns_name(&s.host, 253), || format!("{name}.host"))?;
            ensure(s.port.is_none_or(|p| p >= 1), || format!("{name}.port"))?;
        }
    }
    if let Some(pem) = &e.ca_certificate {
        ensure(is_pem(pem), || "email.ca_certificate".into())?;
    }
    ensure(
        (1..=3).contains(&e.sign_in_methods.len()) && distinct(&e.sign_in_methods).is_ok(),
        || "email.sign_in_methods".into(),
    )?;
    ensure((1..=20).contains(&e.people_link_folders.len()), || {
        "email.people_link_folders".into()
    })?;
    for (i, f) in e.people_link_folders.iter().enumerate() {
        ensure(chars_between(f, 1, 255), || {
            format!("email.people_link_folders[{i}]")
        })?;
    }
    ensure(e.company_domains.len() <= 100, || {
        "email.company_domains".into()
    })?;
    for (i, d) in e.company_domains.iter().enumerate() {
        ensure(is_dns_name(d, 253), || {
            format!("email.company_domains[{i}]")
        })?;
    }
    ensure((1..=500).contains(&e.max_recipients), || {
        "email.max_recipients".into()
    })?;
    ensure(e.no_attach_places.len() <= 100, || {
        "email.no_attach_places".into()
    })?;
    for (i, p) in e.no_attach_places.iter().enumerate() {
        ensure(chars_between(p, 1, 1024), || {
            format!("email.no_attach_places[{i}]")
        })?;
    }
    Ok(())
}

fn channels(doc: &OrganisationProfileDocument) -> Checked {
    match distinct(&doc.channels.allowed) {
        Ok(()) => Ok(()),
        Err(i) => Err(ProfileError::content(format!("channels.allowed[{i}]"))),
    }
}

fn telegram(doc: &OrganisationProfileDocument) -> Checked {
    let Some(chats) = &doc.telegram.allowed_chats else {
        return Ok(());
    };
    ensure(chats.len() <= 500, || "telegram.allowed_chats".into())?;
    for (i, c) in chats.iter().enumerate() {
        ensure(*c != 0, || format!("telegram.allowed_chats[{i}]"))?;
    }
    Ok(())
}

fn server(doc: &OrganisationProfileDocument) -> Checked {
    let s = &doc.server;
    match &s.client_certificate_name {
        Some(name) => ensure(chars_between(name, 1, 255), || {
            "server.client_certificate_name".into()
        })?,
        None => ensure(!s.client_certificate_required, || {
            "server.client_certificate_name".into()
        })?,
    }
    ensure((1..=200).contains(&s.triggers.daily_runs), || {
        "server.triggers.daily_runs".into()
    })?;
    match distinct(&s.external_actions_off) {
        Ok(()) => Ok(()),
        Err(i) => Err(ProfileError::content(format!(
            "server.external_actions_off[{i}]"
        ))),
    }
}

fn nats(doc: &OrganisationProfileDocument) -> Checked {
    let n = &doc.nats;
    ensure(n.servers.len() <= 10, || "nats.servers".into())?;
    for (i, s) in n.servers.iter().enumerate() {
        let host = s
            .strip_prefix("tls://")
            .or_else(|| s.strip_prefix("wss://"))
            .unwrap_or("");
        ensure(!host.is_empty() && url::Url::parse(s).is_ok(), || {
            format!("nats.servers[{i}]")
        })?;
    }
    if let Some(pem) = &n.tls.ca_certificate {
        ensure(is_pem(pem), || "nats.tls.ca_certificate".into())?;
    }
    ensure(n.subjects_allowed.len() <= 200, || {
        "nats.subjects_allowed".into()
    })?;
    for (i, subject) in n.subjects_allowed.iter().enumerate() {
        ensure(
            (1..=255).contains(&subject.len())
                && subject.bytes().all(|b| {
                    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'*' | b'>' | b'-')
                }),
            || format!("nats.subjects_allowed[{i}]"),
        )?;
    }
    Ok(())
}

fn craft(doc: &OrganisationProfileDocument) -> Checked {
    let c = &doc.craft;
    ensure(c.apps.len() <= 50, || "craft.apps".into())?;
    let mut ids = HashSet::new();
    for (i, app) in c.apps.iter().enumerate() {
        let at = |field: &str| format!("craft.apps[{i}].{field}");
        ensure(
            is_app_id(&app.app_id) && ids.insert(app.app_id.as_str()),
            || at("app_id"),
        )?;
        ensure(chars_between(&app.version, 1, 64), || at("version"))?;
        if let Some(tools) = &app.allowed_tools {
            ensure(tools.len() <= 200, || at("allowed_tools"))?;
            for (j, t) in tools.iter().enumerate() {
                ensure(chars_between(t, 1, 128), || {
                    format!("craft.apps[{i}].allowed_tools[{j}]")
                })?;
            }
        }
        ensure((1..=4).contains(&app.platforms.len()), || at("platforms"))?;
        for (platform, asset) in &app.platforms {
            let at =
                |field: &str| format!("craft.apps[{i}].platforms.{}.{field}", serde_name(platform));
            ensure(is_https(&asset.asset_url), || at("asset_url"))?;
            ensure(is_file_name(&asset.file_name), || at("file_name"))?;
        }
    }
    if let Some(mirror) = &c.mirror_url {
        ensure(is_https(mirror), || "craft.mirror_url".into())?;
    }
    for (extension, app) in &c.file_types {
        let at = || format!("craft.file_types.{extension}");
        ensure(
            (1..=16).contains(&extension.len())
                && extension
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
            at,
        )?;
        ensure(ids.contains(app.as_str()), at)?;
    }
    Ok(())
}

/// `^[a-z0-9][a-z0-9-]{0,39}$`.
fn is_app_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=40).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn gitlab(doc: &OrganisationProfileDocument) -> Checked {
    let Some(g) = &doc.gitlab else {
        return Ok(());
    };
    ensure(is_dns_name(&g.host, 253), || "gitlab.host".into())?;
    if let Some(client_id) = &g.client_id {
        ensure(chars_between(client_id, 1, 256), || {
            "gitlab.client_id".into()
        })?;
    }
    Ok(())
}

fn language_servers(doc: &OrganisationProfileDocument) -> Checked {
    let list = &doc.language_servers;
    ensure(list.len() <= 4, || "language_servers".into())?;
    for (i, entry) in list.iter().enumerate() {
        ensure(
            !list[..i].iter().any(|o| o.language == entry.language),
            || "language_servers".into(),
        )?;
    }
    for (i, entry) in list.iter().enumerate() {
        let at = |field: &str| format!("language_servers[{i}].{field}");
        ensure(is_server_name(&entry.name), || at("name"))?;
        ensure(
            chars_between(&entry.version, 1, 64) && !entry.version.contains('/'),
            || at("version"),
        )?;
        ensure((1..=4).contains(&entry.platforms.len()), || at("platforms"))?;
        for (platform, asset) in &entry.platforms {
            let at = |field: &str| {
                format!(
                    "language_servers[{i}].platforms.{}.{field}",
                    serde_name(platform)
                )
            };
            ensure(is_https(&asset.asset_url), || at("asset_url"))?;
            ensure(is_file_name(&asset.file_name), || at("file_name"))?;
            ensure(is_binary_path(&asset.binary_path), || at("binary_path"))?;
        }
    }
    Ok(())
}

/// `^[a-z0-9][a-z0-9_.-]{0,63}$`.
fn is_server_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-')
        })
}

/// 1–255 characters, relative, `/`-separated, no `..`.
fn is_binary_path(value: &str) -> bool {
    chars_between(value, 1, 255)
        && !value.starts_with('/')
        && !value.contains('\\')
        && value.split('/').all(|segment| segment != "..")
}

fn engineering(doc: &OrganisationProfileDocument) -> Checked {
    for (i, kind) in doc
        .engineering
        .github
        .always_approved_kinds
        .iter()
        .enumerate()
    {
        ensure(GITHUB_WRITE_KINDS.contains(&kind.as_str()), || {
            format!("engineering.github.always_approved_kinds[{i}]")
        })?;
    }
    ensure(doc.engineering.coding_tasks.max_attempts >= 1, || {
        "engineering.coding_tasks.max_attempts".into()
    })
}

fn workflows(doc: &OrganisationProfileDocument) -> Checked {
    let Some(r) = &doc.workflows.company_repository else {
        return Ok(());
    };
    let base = "workflows.company_repository";
    ensure(is_dns_name(&r.host, 253), || format!("{base}.host"))?;
    ensure(is_repository(&r.repository), || {
        format!("{base}.repository")
    })?;
    ensure(
        chars_between(&r.folder, 1, 255) && !r.folder.contains(".."),
        || format!("{base}.folder"),
    )
}

fn brand(doc: &OrganisationProfileDocument) -> Checked {
    let Some(b) = &doc.brand else {
        return Ok(());
    };
    ensure(is_key(&b.brand_identifier), || {
        "brand.brand_identifier".into()
    })?;
    ensure(is_version(&b.version), || "brand.version".into())?;
    ensure(!b.strings.is_empty(), || "brand.strings".into())?;
    for (locale, strings) in &b.strings {
        let at = |field: &str| format!("brand.strings.{locale}{field}");
        ensure(is_locale_tag(locale), || at(""))?;
        ensure(chars_between(&strings.product_name, 1, 60), || {
            at(".product_name")
        })?;
        ensure(chars_between(&strings.coordinator_name, 1, 60), || {
            at(".coordinator_name")
        })?;
        ensure(chars_between(&strings.about_text, 1, 8000), || {
            at(".about_text")
        })?;
    }
    for (locale, text) in &b.legal_text {
        ensure(
            is_locale_tag(locale) && chars_between(text, 1, 8000),
            || format!("brand.legal_text.{locale}"),
        )?;
    }
    let mut assets: Vec<(String, &BrandAsset)> = vec![
        ("brand.logos.light".into(), &b.logos.light),
        ("brand.logos.dark".into(), &b.logos.dark),
        ("brand.icon_set.light".into(), &b.icon_set.light),
        ("brand.icon_set.dark".into(), &b.icon_set.dark),
    ];
    ensure(b.fonts.len() <= 20, || "brand.fonts".into())?;
    assets.extend(
        b.fonts
            .iter()
            .enumerate()
            .map(|(i, f)| (format!("brand.fonts[{i}]"), f)),
    );
    for (path, asset) in &assets {
        ensure(is_https(&asset.url), || format!("{path}.url"))?;
        ensure(is_media_type(&asset.media_type), || {
            format!("{path}.media_type")
        })?;
    }
    ensure(b.design_tokens.len() <= 500, || {
        "brand.design_tokens".into()
    })?;
    for (name, value) in &b.design_tokens {
        ensure(value.chars().count() <= 256, || {
            format!("brand.design_tokens.{name}")
        })?;
    }
    for (name, link) in [
        ("brand.links.help_url", &b.links.help_url),
        ("brand.links.support_url", &b.links.support_url),
        ("brand.links.privacy_url", &b.links.privacy_url),
    ] {
        ensure(is_https(link), || name.into())?;
    }
    Ok(())
}

/// `type/subtype`, at most 255 characters.
fn is_media_type(value: &str) -> bool {
    value.len() <= 255
        && value.split_once('/').is_some_and(|(t, s)| {
            !t.is_empty()
                && !s.is_empty()
                && !s.contains('/')
                && value.bytes().all(|b| b.is_ascii_graphic())
        })
}

fn locales(doc: &OrganisationProfileDocument) -> Checked {
    let l = &doc.locales;
    ensure(l.offered.len() <= 50, || "locales.offered".into())?;
    for (i, tag) in l.offered.iter().enumerate() {
        ensure(is_locale_tag(tag) && !l.offered[..i].contains(tag), || {
            format!("locales.offered[{i}]")
        })?;
    }
    ensure(
        is_locale_tag(&l.fallback) && (l.offered.is_empty() || l.offered.contains(&l.fallback)),
        || "locales.fallback".into(),
    )
}

fn updates(doc: &OrganisationProfileDocument) -> Checked {
    match &doc.updates {
        Some(u) => ensure(is_https(&u.feed_url), || "updates.feed_url".into()),
        None => Ok(()),
    }
}

fn sandbox(doc: &OrganisationProfileDocument) -> Checked {
    ensure(
        (1..=240).contains(&doc.sandbox.session_idle_minutes),
        || "sandbox.session_idle_minutes".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_rule_and_order() {
        assert!(is_version("2026.10.3"));
        assert!(!is_version("1.0"));
        assert!(!is_version("1.0.0.0"));
        assert!(!is_version("1.0.1234567890"));
        assert!(version_parts("2.0.0") > version_parts("1.9.10"));
        assert_eq!(version_parts("x"), None);
    }

    #[test]
    fn helper_rules() {
        assert!(is_dns_name("gitlab.example.com", 253));
        assert!(!is_dns_name("https://gitlab.example.com", 253));
        assert!(!is_dns_name("-a.example", 253));
        assert!(is_git_ref("main"));
        assert!(is_git_ref("release/2026"));
        for bad in [
            "", "a..b", "/a", "a/", "a.lock", ".a", "a b", "a@{b", "@", "a.",
        ] {
            assert!(!is_git_ref(bad), "{bad}");
        }
        assert!(is_relative_folder(""));
        assert!(is_relative_folder("knowledge/security"));
        for bad in ["/a", "a/", "a/../b", "./a", "a//b"] {
            assert!(!is_relative_folder(bad), "{bad}");
        }
        assert!(is_uuid("0f8fad5b-d9cb-469f-a165-70867728950e"));
        assert!(!is_uuid("0f8fad5b"));
        assert!(is_locale_tag("en-GB"));
        assert!(!is_locale_tag("e"));
        assert!(is_slack_channel_id("C0123ABCD45"));
        assert!(!is_slack_channel_id("#agents"));
        assert!(is_slack_member_id("U0123ABCD"));
        assert!(!is_slack_member_id("agent-bot"));
        assert!(is_media_type("image/svg+xml"));
        assert!(!is_media_type("image"));
        assert!(is_binary_path("bin/gopls"));
        assert!(!is_binary_path("/bin/gopls"));
        assert!(!is_binary_path("../gopls"));
        assert!(is_front_door_url("https://k.example.org"));
        assert!(!is_front_door_url("https://k.example.org/"));
        assert!(!is_front_door_url("https://k.example.org#x"));
        assert!(!is_front_door_url("https://u@k.example.org"));
        assert_eq!(
            front_door_key("HTTPS://K.Example.org/Path"),
            "https://k.example.org/Path"
        );
        assert!(is_destination(&HelpDestination {
            kind: HelpDestinationKind::SlackChannel,
            address: "#it-help".into()
        }));
        assert!(is_destination(&HelpDestination {
            kind: HelpDestinationKind::SlackChannel,
            address: "C01234567".into()
        }));
        assert!(!is_destination(&HelpDestination {
            kind: HelpDestinationKind::Email,
            address: "a@b@c".into()
        }));
    }
}
