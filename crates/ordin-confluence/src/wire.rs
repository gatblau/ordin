//! The Confluence response shapes this adapter reads, parsed into typed structs (Security: Confluence
//! content is data), and the small path and time helpers shared by the modules.

use chrono::SubsecRound;
use ordin_types::time::Timestamp;
use serde::Deserialize;

use crate::error::ConfluenceError;
use crate::types::{RemoteItemRef, RemoteVersion};

/// A page in the working copy is `<title>.confluence.xhtml` (Constants).
pub(crate) const PAGE_SUFFIX: &str = ".confluence.xhtml";

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Links {
    pub next: Option<String>,
    pub webui: Option<String>,
}

/// Every v1 and v2 listing: `results` and the link to the next batch.
#[derive(Debug, Deserialize)]
pub(crate) struct Listing<T> {
    #[serde(default = "Vec::new")]
    pub results: Vec<T>,
    #[serde(rename = "_links", default)]
    pub links: Links,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireBy {
    #[serde(rename = "accountId")]
    pub account_id: Option<String>,
}

/// v2 writes `createdAt` and `authorId`; v1 (attachment upload) writes `when` and `by.accountId`.
#[derive(Debug, Deserialize)]
pub(crate) struct WireVersion {
    pub number: u64,
    #[serde(rename = "createdAt", alias = "when")]
    pub created_at: String,
    #[serde(rename = "authorId")]
    pub author_id: Option<String>,
    pub by: Option<WireBy>,
}

impl WireVersion {
    pub fn account(&self) -> Option<&str> {
        self.author_id
            .as_deref()
            .or(self.by.as_ref().and_then(|b| b.account_id.as_deref()))
    }

    /// `changed_by` is filled by the caller that names the author.
    pub fn remote(&self) -> Result<RemoteVersion, ConfluenceError> {
        Ok(RemoteVersion {
            tag: self.number.to_string(),
            changed_at: parse_time(&self.created_at)?,
            changed_by: None,
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct StorageBody {
    pub value: String,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct WireBody {
    pub storage: Option<StorageBody>,
}

/// A page. Children listings carry `id` and `title` only, so `version` is optional here.
#[derive(Debug, Deserialize)]
pub(crate) struct WirePage {
    pub id: String,
    pub title: String,
    #[serde(rename = "spaceId")]
    pub space_id: Option<String>,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub version: Option<WireVersion>,
    pub body: Option<WireBody>,
}

impl WirePage {
    pub fn version(&self) -> Result<&WireVersion, ConfluenceError> {
        self.version
            .as_ref()
            .ok_or(ConfluenceError::ResponseInvalid)
    }

    pub fn storage(&self) -> Option<&str> {
        self.body
            .as_ref()
            .and_then(|b| b.storage.as_ref())
            .map(|s| s.value.as_str())
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireAttachment {
    pub id: String,
    pub title: String,
    #[serde(rename = "fileSize")]
    pub file_size: Option<u64>,
    #[serde(rename = "downloadLink")]
    pub download_link: Option<String>,
    pub version: Option<WireVersion>,
}

impl WireAttachment {
    pub fn version(&self) -> Result<&WireVersion, ConfluenceError> {
        self.version
            .as_ref()
            .ok_or(ConfluenceError::ResponseInvalid)
    }
}

/// `POST …/child/attachment` answers a listing; `…/data` answers the content itself.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum UploadReply {
    Listing { results: Vec<WireAttachment> },
    One(WireAttachment),
}

impl UploadReply {
    pub fn attachment(self) -> Result<WireAttachment, ConfluenceError> {
        match self {
            UploadReply::Listing { results } => results
                .into_iter()
                .next()
                .ok_or(ConfluenceError::ResponseInvalid),
            UploadReply::One(one) => Ok(one),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireSpace {
    pub id: String,
    pub key: String,
    pub name: String,
    #[serde(rename = "_links", default)]
    pub links: Links,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HitContent {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HitSpace {
    pub key: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HitContainer {
    pub title: String,
    #[serde(rename = "displayUrl")]
    pub display_url: String,
}

/// One row of `GET /wiki/rest/api/search`.
#[derive(Debug, Deserialize)]
pub(crate) struct SearchHit {
    pub content: Option<HitContent>,
    pub space: Option<HitSpace>,
    #[serde(rename = "resultGlobalContainer")]
    pub container: Option<HitContainer>,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccessibleResource {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RestrictedUser {
    #[serde(rename = "accountId")]
    pub account_id: String,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RestrictedGroup {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Results<T> {
    #[serde(default = "Vec::new")]
    pub results: Vec<T>,
}

impl<T> Default for Results<T> {
    fn default() -> Self {
        Results {
            results: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Restrictions {
    #[serde(default)]
    pub user: Results<RestrictedUser>,
    #[serde(default)]
    pub group: Results<RestrictedGroup>,
}

/// `GET …/restriction/byOperation/read`.
#[derive(Debug, Deserialize)]
pub(crate) struct WireRestriction {
    #[serde(default)]
    pub restrictions: Restrictions,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PermissionPrincipal {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PermissionOperation {
    pub key: String,
    #[serde(rename = "targetType")]
    pub target_type: String,
}

/// One grant of `GET …/spaces/{spaceId}/permissions`.
#[derive(Debug, Deserialize)]
pub(crate) struct SpacePermission {
    pub principal: PermissionPrincipal,
    pub operation: PermissionOperation,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireUser {
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

pub(crate) fn parse_time(raw: &str) -> Result<Timestamp, ConfluenceError> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|t| Timestamp(t.with_timezone(&chrono::Utc).trunc_subsecs(3)))
        .map_err(|_| ConfluenceError::ResponseInvalid)
}

/// A `/` in a title is replaced by `∕` (U+2215) in paths only.
pub(crate) fn path_title(title: &str) -> String {
    title.replace('/', "\u{2215}")
}

pub(crate) fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

/// `<ancestor titles>/<P title>.confluence.xhtml`.
pub(crate) fn page_path(prefix: &str, title: &str) -> String {
    join(prefix, &format!("{}{PAGE_SUFFIX}", path_title(title)))
}

/// The folder a page's children and attachments sit in: its path without the suffix.
pub(crate) fn page_prefix(path: &str) -> String {
    path.strip_suffix(PAGE_SUFFIX).unwrap_or(path).to_owned()
}

/// `page:<id>` or `attachment:<page id>:<id>` (Data Model).
pub(crate) fn item_key(item: &RemoteItemRef) -> String {
    match item {
        RemoteItemRef::Page { page_id } => format!("page:{page_id}"),
        RemoteItemRef::Attachment {
            page_id,
            attachment_id,
        } => format!("attachment:{page_id}:{attachment_id}"),
    }
}

/// The reverse of [`item_key`]; a row that is neither form is a datastore fault.
pub(crate) fn item_of_key(key: &str) -> Result<RemoteItemRef, ConfluenceError> {
    if let Some(page_id) = key.strip_prefix("page:") {
        return Ok(RemoteItemRef::Page {
            page_id: page_id.to_owned(),
        });
    }
    let (page_id, attachment_id) = key
        .strip_prefix("attachment:")
        .and_then(|rest| rest.split_once(':'))
        .ok_or(ConfluenceError::DatastoreUnavailable)?;
    Ok(RemoteItemRef::Attachment {
        page_id: page_id.to_owned(),
        attachment_id: attachment_id.to_owned(),
    })
}

/// A page id goes into CQL unquoted, so it must be digits only.
pub(crate) fn is_page_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
}

/// Escapes text for a quoted CQL string (`"` and `\`), so no operator from the person reaches a search.
pub(crate) fn cql_quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        if matches!(c, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slash_in_a_title_is_replaced_in_paths_only() {
        assert_eq!(
            page_path("Supplier pages", "Rates/2026"),
            "Supplier pages/Rates\u{2215}2026.confluence.xhtml"
        );
        assert_eq!(page_prefix("A/B.confluence.xhtml"), "A/B");
        assert_eq!(page_prefix("A/b.pdf"), "A/b.pdf");
    }

    #[test]
    fn item_keys_go_both_ways() {
        for item in [
            RemoteItemRef::Page {
                page_id: "4411".into(),
            },
            RemoteItemRef::Attachment {
                page_id: "4411".into(),
                attachment_id: "att9".into(),
            },
        ] {
            assert_eq!(item_of_key(&item_key(&item)).unwrap(), item);
        }
        assert_eq!(
            item_of_key("other").unwrap_err().code(),
            "CONF_DATASTORE_UNAVAILABLE"
        );
    }

    #[test]
    fn cql_text_is_quoted_and_escaped() {
        assert_eq!(cql_quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn page_ids_are_digits() {
        assert!(is_page_id("4411"));
        assert!(!is_page_id(""));
        assert!(!is_page_id("1 OR 1=1"));
    }

    #[test]
    fn a_time_that_does_not_parse_is_a_bad_response() {
        assert_eq!(
            parse_time("yesterday").unwrap_err().code(),
            "CONF_RESPONSE_INVALID"
        );
    }
}
