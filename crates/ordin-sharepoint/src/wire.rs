//! The Graph response shapes this adapter reads, parsed into typed structs (Security: names and text from
//! SharePoint are data), and the conversions to the public types.

use chrono::SubsecRound;
use ordin_types::time::Timestamp;
use serde::Deserialize;

use crate::error::SharePointError;
use crate::types::SharePointVersion;

/// The `$select` of one item's metadata. `SharePointLocation` needs `folder` and `webUrl`.
pub(crate) const ITEM_SELECT: &str =
    "id,name,eTag,cTag,size,lastModifiedDateTime,lastModifiedBy,parentReference,file,folder,webUrl";
pub(crate) const CHILDREN_SELECT: &str =
    "id,name,folder,file,size,eTag,cTag,lastModifiedDateTime,lastModifiedBy,parentReference";

#[derive(Debug, Deserialize)]
pub(crate) struct ListPage<T> {
    #[serde(default = "Vec::new")]
    pub value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    pub delta_link: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DriveItem {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "eTag")]
    pub etag: Option<String>,
    #[serde(rename = "cTag")]
    pub ctag: Option<String>,
    pub size: Option<u64>,
    #[serde(rename = "lastModifiedDateTime")]
    pub last_modified: Option<String>,
    #[serde(rename = "lastModifiedBy")]
    pub last_modified_by: Option<IdentitySet>,
    #[serde(rename = "parentReference")]
    pub parent: Option<ParentReference>,
    pub folder: Option<serde_json::Value>,
    pub deleted: Option<serde_json::Value>,
    #[serde(rename = "webUrl")]
    pub web_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ParentReference {
    #[serde(rename = "driveId")]
    pub drive_id: Option<String>,
    #[serde(rename = "siteId")]
    pub site_id: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct IdentityRef {
    pub id: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct IdentitySet {
    pub user: Option<IdentityRef>,
    pub group: Option<IdentityRef>,
    #[serde(rename = "siteGroup")]
    pub site_group: Option<IdentityRef>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Site {
    pub id: String,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Drive {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "webUrl")]
    pub web_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct VersionEntry {
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Permission {
    #[serde(rename = "grantedToV2")]
    pub granted_to: Option<IdentitySet>,
    #[serde(rename = "grantedToIdentitiesV2", default)]
    pub granted_to_identities: Vec<IdentitySet>,
    pub link: Option<PermissionLink>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PermissionLink {
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UploadSession {
    #[serde(rename = "uploadUrl")]
    pub upload_url: String,
}

/// Parses a Graph timestamp (`2026-01-10T13:40:00Z`, with or without fractions) to the wire form.
pub(crate) fn parse_time(raw: &str) -> Result<Timestamp, SharePointError> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|t| Timestamp(t.with_timezone(&chrono::Utc).trunc_subsecs(3)))
        .map_err(|_| SharePointError::ResponseInvalid)
}

impl DriveItem {
    pub fn is_folder(&self) -> bool {
        self.folder.is_some()
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted.is_some()
    }

    /// `lastModifiedBy.user.displayName`.
    pub fn changed_by(&self) -> Option<String> {
        self.last_modified_by
            .as_ref()
            .and_then(|by| by.user.as_ref())
            .and_then(|user| user.display_name.clone())
    }

    pub fn changed_at(&self) -> Result<Timestamp, SharePointError> {
        parse_time(
            self.last_modified
                .as_deref()
                .ok_or(SharePointError::ResponseInvalid)?,
        )
    }

    /// The version of a file: `cTag`, `eTag`, time and author; both tags are required.
    pub fn version(&self, label: Option<String>) -> Result<SharePointVersion, SharePointError> {
        let tag = self.ctag.clone().filter(|t| !t.is_empty());
        let etag = self.etag.clone().filter(|t| !t.is_empty());
        match (tag, etag) {
            (Some(tag), Some(etag)) => Ok(SharePointVersion {
                tag,
                etag,
                label,
                changed_at: self.changed_at()?,
                changed_by: self.changed_by(),
            }),
            _ => Err(SharePointError::ResponseInvalid),
        }
    }

    /// The folder path above this item from the library root, `/`-separated with no outer slashes
    /// (`""` for a child of the root). `None` when Graph sent no `parentReference.path`.
    pub fn parent_path(&self) -> Option<String> {
        let raw = self.parent.as_ref()?.path.as_deref()?;
        let (_, rest) = raw.split_once("root:")?;
        Some(rest.trim_matches('/').to_owned())
    }

    /// The item's own path from the library root.
    pub fn full_path(&self) -> Option<String> {
        let parent = self.parent_path()?;
        Some(join_path(&parent, &self.name))
    }
}

/// `a` + `b` with one `/`; either may be empty.
pub(crate) fn join_path(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_owned(),
        (_, true) => a.to_owned(),
        _ => format!("{a}/{b}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: serde_json::Value) -> DriveItem {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn paths_are_taken_after_the_root_marker() {
        let nested = item(serde_json::json!({
            "id": "1", "name": "Kestrel MSA.docx",
            "parentReference": {"path": "/drives/b!x/root:/Suppliers/Contracts"}
        }));
        assert_eq!(nested.parent_path().as_deref(), Some("Suppliers/Contracts"));
        assert_eq!(
            nested.full_path().as_deref(),
            Some("Suppliers/Contracts/Kestrel MSA.docx")
        );
        let top = item(serde_json::json!({
            "id": "1", "name": "a.docx", "parentReference": {"path": "/drives/b!x/root:"}
        }));
        assert_eq!(top.full_path().as_deref(), Some("a.docx"));
        let none = item(serde_json::json!({"id": "1", "name": "a"}));
        assert_eq!(none.full_path(), None);
    }

    #[test]
    fn a_version_needs_both_tags_and_a_time() {
        let ok = item(serde_json::json!({
            "id": "1", "name": "a", "eTag": "e", "cTag": "c",
            "lastModifiedDateTime": "2026-01-10T13:40:00Z",
            "lastModifiedBy": {"user": {"displayName": "Sam Lee"}}
        }));
        let version = ok.version(Some("4.0".into())).unwrap();
        assert_eq!(version.tag, "c");
        assert_eq!(version.changed_by.as_deref(), Some("Sam Lee"));
        assert_eq!(version.changed_at.to_wire(), "2026-01-10T13:40:00.000Z");
        let missing = item(serde_json::json!({"id": "1", "name": "a", "eTag": "e"}));
        assert!(missing.version(None).is_err());
        assert_eq!(join_path("", "x"), "x");
        assert_eq!(join_path("a", ""), "a");
        assert_eq!(join_path("a", "b"), "a/b");
        assert!(parse_time("yesterday").is_err());
    }
}
