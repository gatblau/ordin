//! The adapter's construction and the helpers every method shares: item metadata, version labels, the
//! location behind an item, and the external-action bracket.

use std::sync::Arc;
#[cfg(any(test, feature = "test-support"))]
use std::time::Duration;

use ordin_identity::IdentityService;
use ordin_journal::{Journal, NewExternalAction};
use ordin_types::CodedError as _;
use sqlx::SqlitePool;

use crate::error::SharePointError;
use crate::graph::{Graph, Req};
use crate::types::{ExternalWrite, SharePointItemRef, SharePointLocation, SharePointVersion};
use crate::wire::{join_path, Drive, DriveItem, ListPage, Site, VersionEntry, ITEM_SELECT};

/// 250 MiB per fetch or upload (*assumed here*: bounds memory).
pub(crate) const MAX_FILE_BYTES: u64 = 250 * 1024 * 1024;
/// Graph's simple-upload limit.
pub(crate) const SIMPLE_UPLOAD_MAX: usize = 4 * 1024 * 1024;
/// A multiple of 320 KiB, as Graph requires.
pub(crate) const UPLOAD_RANGE: usize = 10 * 1024 * 1024;
/// 100,000 items and 20 folder levels per link (A-21).
pub(crate) const MAX_LIST_ITEMS: usize = 100_000;
pub(crate) const MAX_FOLDER_LEVELS: usize = 20;
/// At most 50 locations per `find_locations`.
pub(crate) const FIND_LIMIT: u32 = 50;

pub struct SharePointDeps {
    pub identity: Arc<IdentityService>,
    /// Also the source of this crate's `Datastore` (`journal.datastore()`, D4).
    pub journal: Journal,
    /// Built by the program with SA-HTTP-02's TLS and proxy settings.
    pub http: reqwest::Client,
}

pub struct SharePointAdapter {
    pub(crate) graph: Graph,
    pub(crate) journal: Journal,
    pub(crate) pool: SqlitePool,
}

impl SharePointAdapter {
    /// Fails with `SP_DATASTORE_UNAVAILABLE` when the Journal's datastore is not SQLite (this crate is not
    /// built into the agent profile).
    // spec: ordin-desktop/components/SharePointAdapter#SA-RECON-03
    pub fn new(deps: SharePointDeps) -> Result<SharePointAdapter, SharePointError> {
        let pool = deps
            .journal
            .datastore()
            .sqlite()
            .cloned()
            .ok_or(SharePointError::DatastoreUnavailable)?;
        Ok(SharePointAdapter {
            graph: Graph::new(deps.identity, deps.http)?,
            journal: deps.journal,
            pool,
        })
    }

    // Points the adapter at a loopback stand-in, scales every wait and sets the transfer and metadata
    // timeouts, so a test need not sit through 1 s, 2 s, 4 s, 30 s and 120 s.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_stand_in(
        mut self,
        graph_base: &str,
        wait_scale: f64,
        transfer_timeout: Duration,
    ) -> Result<Self, SharePointError> {
        self.graph
            .use_stand_in(graph_base, wait_scale, transfer_timeout)?;
        Ok(self)
    }

    /// One item's metadata; `None` is the library's root.
    pub(crate) async fn item_meta(
        &self,
        drive_id: &str,
        item_id: Option<&str>,
    ) -> Result<DriveItem, SharePointError> {
        let url = match item_id {
            Some(id) => self.graph.url(
                &["drives", drive_id, "items", id],
                &[("$select", ITEM_SELECT)],
            ),
            None => self
                .graph
                .url(&["drives", drive_id, "root"], &[("$select", ITEM_SELECT)]),
        };
        self.graph
            .call(Req::get("item_metadata", url))
            .await?
            .json()
    }

    /// The latest SharePoint version label; absent when Graph has none or the person cannot read versions.
    pub(crate) async fn latest_label(
        &self,
        drive_id: &str,
        item_id: &str,
    ) -> Result<Option<String>, SharePointError> {
        let url = self.graph.url(
            &["drives", drive_id, "items", item_id, "versions"],
            &[("$top", "1"), ("$select", "id,lastModifiedDateTime")],
        );
        match self.graph.call(Req::get("item_versions", url)).await {
            Ok(resp) => {
                let page: ListPage<VersionEntry> = resp.json()?;
                Ok(page.value.into_iter().next().and_then(|v| v.id))
            }
            Err(SharePointError::NotFound | SharePointError::AccessDenied) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Metadata of `item` and its latest version label.
    pub(crate) async fn version_of(
        &self,
        item: &SharePointItemRef,
    ) -> Result<(DriveItem, SharePointVersion), SharePointError> {
        let meta = self.item_meta(&item.drive_id, Some(&item.item_id)).await?;
        let label = self.latest_label(&item.drive_id, &item.item_id).await?;
        let version = meta.version(label)?;
        Ok((meta, version))
    }

    /// The location of `item`: its drive's name and its site's name are read for the rebuild
    /// (Internal Logic 2.2). A library root has no folder path above it and a folder facet.
    // The site id is `parentReference.siteId` of the item; a reply without it is
    // `SP_RESPONSE_INVALID`.
    pub(crate) async fn location_of(
        &self,
        item: &DriveItem,
        drive_id: &str,
    ) -> Result<SharePointLocation, SharePointError> {
        let site_id = item
            .parent
            .as_ref()
            .and_then(|p| p.site_id.clone())
            .ok_or(SharePointError::ResponseInvalid)?;
        let drive: Drive = self
            .graph
            .call(Req::get(
                "drive_metadata",
                self.graph
                    .url(&["drives", drive_id], &[("$select", "id,name,webUrl")]),
            ))
            .await?
            .json()?;
        let site: Site = self
            .graph
            .call(Req::get(
                "site_metadata",
                self.graph.url(
                    &["sites", &site_id],
                    &[("$select", "id,displayName,webUrl")],
                ),
            ))
            .await?
            .json()?;
        let is_root = item.parent_path().is_none() && item.is_folder();
        Ok(SharePointLocation {
            site_id: site.id,
            site_name: site.display_name,
            drive_id: drive_id.to_owned(),
            drive_name: drive.name,
            item_id: if is_root { None } else { Some(item.id.clone()) },
            path: if is_root {
                String::new()
            } else {
                item.full_path().unwrap_or_else(|| item.name.clone())
            },
            is_folder: item.is_folder(),
            web_url: item.web_url.clone().or(drive.web_url).unwrap_or_default(),
        })
    }

    /// Internal Logic 7.2: `SharePoint › <site> › <library> › <path>`, within the Journal's 2,048 characters.
    pub(crate) fn destination(location: &SharePointLocation) -> String {
        let text = format!(
            "SharePoint \u{203A} {} \u{203A} {} \u{203A} {}",
            location.site_name, location.drive_name, location.path
        );
        text.trim_end().chars().take(2048).collect()
    }

    /// Step 6.3: begins the one external action of a write.
    // spec: ordin-desktop/components/SharePointAdapter#SA-RECON-02
    pub(crate) async fn begin(
        &self,
        action: &ExternalWrite,
        destination: String,
    ) -> Result<(), SharePointError> {
        match self
            .journal
            .begin_external_action(NewExternalAction {
                action_key: action.key,
                run_id: action.run_id,
                kind: action.kind,
                destination,
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) if e.code() == "EXT_KEY_DUPLICATE" => Err(SharePointError::ActionKeyUsed),
            // `JRNL_DATASTORE_UNAVAILABLE`, and any other refusal: nothing is sent.
            Err(_) => Err(SharePointError::DatastoreUnavailable),
        }
    }

    /// The path segments that address `folder` as a parent: `drives/<d>/items/<id>:` or `drives/<d>/root:`.
    pub(crate) fn parent_segments(folder: &SharePointLocation) -> Vec<String> {
        match &folder.item_id {
            Some(id) => vec![
                "drives".to_owned(),
                folder.drive_id.clone(),
                "items".to_owned(),
                format!("{id}:"),
            ],
            None => vec![
                "drives".to_owned(),
                folder.drive_id.clone(),
                "root:".to_owned(),
            ],
        }
    }

    /// The path of a child of `folder`, relative to the library root.
    pub(crate) fn child_path(folder: &SharePointLocation, name: &str) -> String {
        join_path(&folder.path, name)
    }
}

/// SharePoint's forbidden characters (Internal Logic 5.1): 1–255 characters, none of `/ \ : * ? " < > |`.
pub(crate) fn valid_name(name: &str) -> bool {
    let length = name.chars().count();
    (1..=255).contains(&length)
        && !name
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_sharepoints_forbidden_characters() {
        assert!(valid_name("Kestrel MSA.docx"));
        assert!(valid_name(&"a".repeat(255)));
        assert!(!valid_name(""));
        assert!(!valid_name(&"a".repeat(256)));
        for bad in [
            "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b",
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn destinations_are_cut_to_the_journals_limit() {
        let location = SharePointLocation {
            site_id: "s".into(),
            site_name: "Procurement".into(),
            drive_id: "d".into(),
            drive_name: "Documents".into(),
            item_id: None,
            path: "Rates/2026".into(),
            is_folder: true,
            web_url: String::new(),
        };
        assert_eq!(
            SharePointAdapter::destination(&location),
            "SharePoint \u{203A} Procurement \u{203A} Documents \u{203A} Rates/2026"
        );
        let long = SharePointLocation {
            path: "x".repeat(5000),
            ..location
        };
        assert_eq!(SharePointAdapter::destination(&long).chars().count(), 2048);
    }
}
