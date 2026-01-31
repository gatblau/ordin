//! Address forms (JiraAdapter.md *Address forms*; Internal Logic 2.1). Parsing makes no network call.

use crate::{JiraError, JiraIssueRef, JiraProjectRef};

/// The whole address is at most this many characters.
const MAX_ADDRESS_CHARS: usize = 2048;

/// `^[a-z0-9][a-z0-9-]{0,62}\.atlassian\.net$` over an already lower-cased host.
pub(crate) fn site_ok(host: &str) -> bool {
    let Some(name) = host.strip_suffix(".atlassian.net") else {
        return false;
    };
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first_ok = bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit();
    first_ok
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// `^[A-Z][A-Z0-9_]{1,254}$`.
pub(crate) fn project_key_ok(key: &str) -> bool {
    let bytes = key.as_bytes();
    (2..=255).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
}

/// `^[1-9][0-9]{0,9}$`.
fn number_ok(n: &str) -> bool {
    let bytes = n.as_bytes();
    (1..=10).contains(&bytes.len()) && bytes[0] != b'0' && bytes.iter().all(u8::is_ascii_digit)
}

/// `^[A-Z][A-Z0-9_]{1,254}-[1-9][0-9]{0,9}$`.
pub(crate) fn issue_key_ok(key: &str) -> bool {
    match key.rsplit_once('-') {
        Some((project, n)) => project_key_ok(project) && number_ok(n),
        None => false,
    }
}

/// The site and the remaining URL, after the scheme, length, host and authority rules.
fn parse_common(address: &str) -> Result<(String, url::Url), JiraError> {
    if address.chars().count() > MAX_ADDRESS_CHARS {
        return Err(JiraError::AddressInvalid);
    }
    let url = url::Url::parse(address).map_err(|_| JiraError::AddressInvalid)?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(JiraError::AddressInvalid);
    }
    // `Url` hides a default port, so the authority is checked as written.
    let authority = address
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    if authority.contains(':') || authority.contains('@') {
        return Err(JiraError::AddressInvalid);
    }
    let host = url
        .host_str()
        .ok_or(JiraError::AddressInvalid)?
        .to_ascii_lowercase();
    if !site_ok(&host) {
        return Err(JiraError::AddressInvalid);
    }
    Ok((host, url))
}

/// The path segments with empty ones (a trailing slash) dropped.
fn segments(url: &url::Url) -> Vec<String> {
    url.path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The project key of the project forms; `None` for a path that is not one.
fn project_from_path(parts: &[String]) -> Option<String> {
    let at = |i: usize| parts.get(i).map(String::as_str);
    let key = match (at(0), at(1), at(2), at(3)) {
        (Some("projects"), Some(k), _, _) => Some(k),
        (Some("jira"), Some("software" | "core"), Some("projects"), Some(k)) => Some(k),
        (Some("jira"), Some("software"), Some("c"), Some("projects")) => at(4),
        (Some("browse"), Some(k), None, _) => Some(k),
        _ => None,
    }?;
    let upper = key.to_ascii_uppercase();
    project_key_ok(&upper).then_some(upper)
}

/// Parses a Jira Cloud project address into its site and upper-cased key.
pub(crate) fn parse_project(address: &str) -> Result<JiraProjectRef, JiraError> {
    let (site, url) = parse_common(address)?;
    let key = project_from_path(&segments(&url)).ok_or(JiraError::AddressInvalid)?;
    Ok(JiraProjectRef { site, key })
}

/// Parses a Jira Cloud issue address into its site and upper-cased key.
pub(crate) fn parse_issue(address: &str) -> Result<JiraIssueRef, JiraError> {
    let (site, url) = parse_common(address)?;
    let parts = segments(&url);
    if parts.first().map(String::as_str) == Some("browse") {
        let key = parts
            .get(1)
            .map(|k| k.to_ascii_uppercase())
            .ok_or(JiraError::AddressInvalid)?;
        // A bare project in `/browse/{KEY}` is not an issue.
        return if issue_key_ok(&key) {
            Ok(JiraIssueRef { site, key })
        } else {
            Err(JiraError::AddressInvalid)
        };
    }
    if project_from_path(&parts).is_none() {
        return Err(JiraError::AddressInvalid);
    }
    let selected = url
        .query_pairs()
        .find(|(name, _)| name == "selectedIssue")
        .map(|(_, value)| value.to_ascii_uppercase())
        .ok_or(JiraError::AddressInvalid)?;
    if issue_key_ok(&selected) {
        Ok(JiraIssueRef {
            site,
            key: selected,
        })
    } else {
        Err(JiraError::AddressInvalid)
    }
}

pub(crate) fn project_address(project: &JiraProjectRef) -> String {
    format!("https://{}/projects/{}", project.site, project.key)
}

pub(crate) fn issue_address(issue: &JiraIssueRef) -> String {
    format!("https://{}/browse/{}", issue.site, issue.key)
}
