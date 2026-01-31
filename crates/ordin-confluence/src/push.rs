//! Pushing a page or attachment (ConfluenceAdapter.md Internal Logic 7). Each write is one external
//! action begun here before anything is sent and confirmed here after Confluence accepts it; none is
//! resent automatically after an unknown outcome.

use bytes::Bytes;
use ordin_journal::NewExternalAction;
use ordin_types::domain::places::CopyDestination;
use ordin_types::domain::runs::ExternalActionKind;
use ordin_types::{CodedError as _, ExternalActionKey, RunId};
use reqwest::Method;
use serde_json::json;

use crate::client::{relative, Failure, Req, Resp};
use crate::convert::{banner_macro, check_xml};
use crate::error::ConfluenceError;
use crate::store;
use crate::track::version_number;
use crate::types::{RemoteItemRef, RemoteVersion};
use crate::wire::{item_key, UploadReply, WireAttachment, WirePage};
use crate::ConfluenceAdapter;

/// 5 MiB of storage XHTML per page (*assumed here*).
pub(crate) const MAX_PAGE_BODY_BYTES: usize = 5 * 1024 * 1024;
/// 100 MiB (*assumed here*: Confluence Cloud's default attachment limit).
pub(crate) const MAX_ATTACHMENT_BYTES: u64 = 100 * 1024 * 1024;
/// A page update's `version.message`.
const VERSION_MESSAGE: &str = "Updated from ordin";

/// What one write needs for its external action.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bracket {
    pub key: ExternalActionKey,
    pub run_id: Option<RunId>,
    pub kind: ExternalActionKind,
}

/// `Confluence › {space} › {page title}`, within the Journal's 2,048 characters.
fn destination(space: &str, title: &str) -> String {
    format!("Confluence \u{203A} {space} \u{203A} {title}")
        .chars()
        .take(2048)
        .collect()
}

/// The `{space}` of a destination when only the page is at hand.
// The surface table has no read of a space by id, so the space's id stands in for its name
// in the external action's destination line; `publish_page` knows the key and writes that.
fn space_of(page: &WirePage) -> &str {
    page.space_id.as_deref().unwrap_or_default()
}

/// One `file` part. A quote or line break in a name cannot end the header.
fn multipart(file_name: &str, bytes: &[u8]) -> (String, Bytes) {
    let boundary = format!("ordin-{}", uuid::Uuid::new_v4().simple());
    let name: String = file_name
        .chars()
        .map(|c| {
            if matches!(c, '"' | '\r' | '\n') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut body = Vec::with_capacity(bytes.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (
        format!("multipart/form-data; boundary={boundary}"),
        Bytes::from(body),
    )
}

impl ConfluenceAdapter {
    /// Step 7.1: the only record of the write.
    // spec: ordin-desktop/components/ConfluenceAdapter#SA-RECON-01
    async fn begin(&self, bracket: Bracket, destination: String) -> Result<(), ConfluenceError> {
        match self
            .journal
            .begin_external_action(NewExternalAction {
                action_key: bracket.key,
                run_id: bracket.run_id,
                kind: bracket.kind,
                destination,
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) if e.code() == "EXT_KEY_DUPLICATE" => Err(ConfluenceError::ActionKeyUsed),
            Err(_) => Err(ConfluenceError::DatastoreUnavailable),
        }
    }

    /// Confirms the external action. A failure after Confluence accepted the write is logged and does not
    /// fail the write: the Journal's start-up recovery settles a `sent` row.
    async fn settle(&self, key: ExternalActionKey) {
        if self.journal.confirm_external_action(key).await.is_err() {
            tracing::warn!(
                component = "confluence",
                action_key = key.to_string().as_str(),
                "confluence_confirm_failed"
            );
        }
    }

    /// Logs `confluence_outcome_unknown`; the external action stays `sent` for the recovery.
    fn outcome_unknown(&self, key: ExternalActionKey) -> ConfluenceError {
        tracing::warn!(
            component = "confluence",
            action_key = key.to_string().as_str(),
            "confluence_outcome_unknown"
        );
        ConfluenceError::OutcomeUnknown
    }

    /// Step 7.6: the push is ordin's own, so the next check must not report it (best effort: if this fails
    /// the push is reported once).
    async fn remember(&self, item: &RemoteItemRef, version: &RemoteVersion) {
        let Ok(pool) = self.pool() else { return };
        if let Ok(number) = version_number(&version.tag) {
            let _ = store::update_item_version(pool, &item_key(item), number, version).await;
        }
    }

    /// Step 7.6: sends a write whose item has a version to compare. A timeout, a lost connection, a 5xx,
    /// 429 or 503 reads the item; the version still the expected one proves nothing landed, and the write
    /// goes once more with the same key.
    // spec: ordin-desktop/components/ConfluenceAdapter#CONF_OUTCOME_UNKNOWN
    async fn send_checked(
        &self,
        make: impl Fn() -> Req,
        item: &RemoteItemRef,
        expected: &str,
        key: ExternalActionKey,
    ) -> Result<Resp, ConfluenceError> {
        let mut retried = false;
        loop {
            let failure = match self.transport.send(make()).await {
                Ok(reply) => return Ok(reply),
                Err(Failure::Error(e)) => return Err(e),
                Err(failure) => failure,
            };
            let unmoved = matches!(
                self.version_read(item).await,
                Ok(read) if read.tag == expected
            );
            match (unmoved, retried, failure) {
                (true, false, _) => {
                    retried = true;
                    self.transport.pause().await;
                }
                (true, true, Failure::Busy(e)) => return Err(e),
                _ => return Err(self.outcome_unknown(key)),
            }
        }
    }

    /// Sends a create, which has no version to compare. A 429 refused it; anything else may have landed.
    async fn send_create(&self, req: Req, key: ExternalActionKey) -> Result<Resp, ConfluenceError> {
        match self.transport.send(req).await {
            Ok(reply) => Ok(reply),
            Err(Failure::Error(e) | Failure::Busy(e @ ConfluenceError::Throttled)) => Err(e),
            Err(_) => Err(self.outcome_unknown(key)),
        }
    }

    pub(crate) fn storage_of(
        markdown: &str,
        banner: Option<&str>,
    ) -> Result<String, ConfluenceError> {
        let mut storage = Self::markdown_to_storage(markdown)?;
        if let Some(banner) = banner {
            storage.insert_str(0, &banner_macro(banner));
        }
        if storage.len() > MAX_PAGE_BODY_BYTES {
            return Err(ConfluenceError::TooLarge);
        }
        Ok(storage)
    }

    /// Step 7.2.
    // spec: ordin-desktop/components/ConfluenceAdapter#ADR-069
    pub async fn publish_page(
        &self,
        dest: &CopyDestination,
        title: &str,
        markdown: &str,
        banner: Option<&str>,
        key: ExternalActionKey,
        run_id: Option<RunId>,
    ) -> Result<(String, RemoteVersion), ConfluenceError> {
        let CopyDestination::ConfluencePage {
            space,
            parent_page_id,
        } = dest
        else {
            return Err(ConfluenceError::LocationInvalid);
        };
        let storage = Self::storage_of(markdown, banner)?;
        let space_id = self.space_by_key(space).await?.id;
        self.begin(
            Bracket {
                key,
                run_id,
                kind: ExternalActionKind::CopyPublish,
            },
            destination(space, title),
        )
        .await?;
        let body = json!({
            "spaceId": space_id,
            "status": "current",
            "title": title,
            "parentId": parent_page_id,
            "body": {"representation": "storage", "value": storage},
        });
        let reply = self
            .send_create(
                Req::new(
                    "create_page",
                    Method::POST,
                    relative(&["wiki", "api", "v2", "pages"], &[]),
                )
                .json(&body)
                .transfer()
                .write(),
                key,
            )
            .await?;
        let page: WirePage = reply.json()?;
        let version = page.version()?.remote()?;
        self.settle(key).await;
        tracing::info!(
            component = "confluence",
            kind = "page",
            created = true,
            "confluence_pushed"
        );
        Ok((page.id, version))
    }

    /// Steps 7.3 and 7.4: the check of the version read, and the only place in ordin that adds 1 to a
    /// Confluence version. Nothing is begun when the check refuses: nothing was sent.
    // spec: ordin-desktop/components/ConfluenceAdapter#SA-RECON-02
    // spec: ordin-desktop/components/ConfluenceAdapter#R19
    pub(crate) async fn put_page(
        &self,
        page_id: &str,
        title: Option<&str>,
        storage: &str,
        expected_version: &str,
        bracket: Bracket,
    ) -> Result<RemoteVersion, ConfluenceError> {
        if storage.len() > MAX_PAGE_BODY_BYTES {
            return Err(ConfluenceError::TooLarge);
        }
        let page = self.read_page(page_id).await?;
        let current = page.version()?.remote()?;
        if current.tag != expected_version {
            return Err(ConfluenceError::OriginChanged { current });
        }
        let title = title.unwrap_or(&page.title);
        self.begin(bracket, destination(space_of(&page), title))
            .await?;
        let next = page.version()?.number + 1;
        let body = json!({
            "id": page_id,
            "status": "current",
            "title": title,
            "body": {"representation": "storage", "value": storage},
            "version": {"number": next, "message": VERSION_MESSAGE},
        });
        let url = relative(&["wiki", "api", "v2", "pages", page_id], &[]);
        let item = RemoteItemRef::Page {
            page_id: page_id.to_owned(),
        };
        let reply = self
            .send_checked(
                || {
                    Req::new("update_page", Method::PUT, url.clone())
                        .json(&body)
                        .transfer()
                        .write()
                        .pass(&[409])
                },
                &item,
                expected_version,
                bracket.key,
            )
            .await?;
        if reply.status == 409 {
            // The external action stays `sent`; the page was not overwritten.
            let current = self.version_read(&item).await?;
            return Err(ConfluenceError::OriginChanged { current });
        }
        let updated: WirePage = reply.json()?;
        let version = updated.version()?.remote()?;
        self.settle(bracket.key).await;
        self.remember(&item, &version).await;
        tracing::info!(
            component = "confluence",
            kind = "page",
            created = false,
            "confluence_pushed"
        );
        Ok(version)
    }

    /// `expected_version` is the page version the caller **read** (`RemoteVersion.tag`, e.g. "7"), never
    /// that number plus one.
    // The Public Interface fixes these parameters.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_page(
        &self,
        page_id: &str,
        title: &str,
        markdown: &str,
        banner: Option<&str>,
        expected_version: &str,
        key: ExternalActionKey,
        run_id: Option<RunId>,
    ) -> Result<RemoteVersion, ConfluenceError> {
        let storage = Self::storage_of(markdown, banner)?;
        self.put_page(
            page_id,
            Some(title),
            &storage,
            expected_version,
            Bracket {
                key,
                run_id,
                kind: ExternalActionKind::CopyPublish,
            },
        )
        .await
    }

    /// As `update_page`, for a working-copy page sent back to its origin.
    pub async fn update_page_storage(
        &self,
        page_id: &str,
        storage_xhtml: &str,
        expected_version: &str,
        key: ExternalActionKey,
        run_id: Option<RunId>,
    ) -> Result<RemoteVersion, ConfluenceError> {
        check_xml(storage_xhtml)?;
        self.put_page(
            page_id,
            None,
            storage_xhtml,
            expected_version,
            Bracket {
                key,
                run_id,
                kind: ExternalActionKind::OriginPush,
            },
        )
        .await
    }

    /// The attachment called `file_name` on a page. `attach` has only the name to go by.
    async fn attachment_named(
        &self,
        page_id: &str,
        file_name: &str,
    ) -> Result<WireAttachment, ConfluenceError> {
        self.attachments(page_id)
            .await?
            .into_iter()
            .find(|a| a.title == file_name)
            .ok_or(ConfluenceError::NotFound)
    }

    /// Step 7.5. Answers the attachment's id and its new version.
    pub async fn attach(
        &self,
        page_id: &str,
        file_name: &str,
        bytes: Vec<u8>,
        expected_version: Option<&str>,
        key: ExternalActionKey,
        run_id: Option<RunId>,
    ) -> Result<(String, RemoteVersion), ConfluenceError> {
        if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
            return Err(ConfluenceError::TooLarge);
        }
        let page = self.read_page(page_id).await?;
        let existing = match expected_version {
            Some(expected) => {
                let found = self.attachment_named(page_id, file_name).await?;
                let current = found.version()?.remote()?;
                if current.tag != expected {
                    return Err(ConfluenceError::OriginChanged { current });
                }
                Some(found.id)
            }
            None => None,
        };
        self.begin(
            Bracket {
                key,
                run_id,
                kind: ExternalActionKind::CopyPublish,
            },
            destination(space_of(&page), &page.title),
        )
        .await?;

        let (content_type, body) = multipart(file_name, &bytes);
        let upload = |op: &'static str, url: String| {
            Req::new(op, Method::POST, url)
                .header("content-type", content_type.clone())
                .header("x-atlassian-token", "no-check")
                .bytes(body.clone())
                .transfer()
                .write()
        };
        let base = [
            "wiki",
            "rest",
            "api",
            "content",
            page_id,
            "child",
            "attachment",
        ];
        let reply = match (&existing, expected_version) {
            (Some(attachment_id), Some(expected)) => {
                let mut segments = base.to_vec();
                segments.push(attachment_id);
                segments.push("data");
                let url = relative(&segments, &[]);
                let item = RemoteItemRef::Attachment {
                    page_id: page_id.to_owned(),
                    attachment_id: attachment_id.clone(),
                };
                self.send_checked(
                    || upload("attachment_version", url.clone()),
                    &item,
                    expected,
                    key,
                )
                .await?
            }
            _ => {
                self.send_create(upload("add_attachment", relative(&base, &[])), key)
                    .await?
            }
        };
        let uploaded = reply.json::<UploadReply>()?.attachment()?;
        let version = uploaded.version()?.remote()?;
        self.settle(key).await;
        self.remember(
            &RemoteItemRef::Attachment {
                page_id: page_id.to_owned(),
                attachment_id: uploaded.id.clone(),
            },
            &version,
        )
        .await;
        tracing::info!(
            component = "confluence",
            kind = "attachment",
            created = existing.is_none(),
            "confluence_pushed"
        );
        Ok((uploaded.id, version))
    }

    /// The title a destination line names for an item.
    async fn title_of(
        &self,
        item: &RemoteItemRef,
    ) -> Result<(String, String, String), ConfluenceError> {
        match item {
            RemoteItemRef::Page { page_id } => {
                let page = self.read_page(page_id).await?;
                let tag = page.version()?.remote()?.tag;
                Ok((space_of(&page).to_owned(), page.title, tag))
            }
            RemoteItemRef::Attachment {
                page_id,
                attachment_id,
            } => {
                let found = self.read_attachment(page_id, attachment_id).await?;
                let tag = found.version()?.remote()?.tag;
                let page = self.read_page(page_id).await?;
                Ok((space_of(&page).to_owned(), found.title, tag))
            }
        }
    }

    /// Step 7.7: moves a page or an attachment to the space's trash; nothing is purged.
    // spec: ordin-desktop/components/ConfluenceAdapter#ADR-069
    pub async fn delete(
        &self,
        item: &RemoteItemRef,
        expected_version: Option<&str>,
        key: ExternalActionKey,
        run_id: Option<RunId>,
    ) -> Result<(), ConfluenceError> {
        let (space, title, tag) = self.title_of(item).await?;
        if expected_version.is_some_and(|expected| expected != tag) {
            return Err(ConfluenceError::OriginChanged {
                current: self.version_read(item).await?,
            });
        }
        self.begin(
            Bracket {
                key,
                run_id,
                kind: ExternalActionKind::CopyPublish,
            },
            destination(&space, &title),
        )
        .await?;
        let url = match item {
            RemoteItemRef::Page { page_id } => {
                relative(&["wiki", "api", "v2", "pages", page_id], &[])
            }
            RemoteItemRef::Attachment { attachment_id, .. } => {
                relative(&["wiki", "api", "v2", "attachments", attachment_id], &[])
            }
        };
        let mut retried = false;
        loop {
            let request = Req::new("delete_item", Method::DELETE, url.clone()).write();
            let failure = match self.transport.send(request).await {
                Ok(_) => break,
                Err(Failure::Error(e)) => return Err(e),
                Err(failure) => failure,
            };
            match (self.version_read(item).await, retried, failure) {
                // Gone already: the DELETE that timed out landed.
                (Err(ConfluenceError::NotFound), _, _) => break,
                (Ok(read), false, _) if read.tag == tag => {
                    retried = true;
                    self.transport.pause().await;
                }
                (Ok(read), true, Failure::Busy(e)) if read.tag == tag => return Err(e),
                _ => return Err(self.outcome_unknown(key)),
            }
        }
        self.settle(key).await;
        if let Ok(pool) = self.pool() {
            // Best effort: if this fails the removal is reported once.
            let _ = store::delete_item(pool, &item_key(item)).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_destination_is_cut_to_the_journals_limit() {
        assert_eq!(
            destination("PROC", "Rates"),
            "Confluence \u{203A} PROC \u{203A} Rates"
        );
        assert_eq!(destination("P", &"x".repeat(3000)).chars().count(), 2048);
    }

    #[test]
    fn a_file_name_cannot_end_the_part_header() {
        let (content_type, body) = multipart("a\"b\r\n.pdf", b"123");
        let boundary = content_type.split("boundary=").nth(1).unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with(&format!("--{boundary}\r\n")));
        assert!(text.contains("filename=\"a_b__.pdf\""));
        assert!(text.ends_with(&format!("\r\n--{boundary}--\r\n")));
    }
}
