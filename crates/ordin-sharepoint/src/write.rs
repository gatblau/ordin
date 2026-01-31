//! Writes (Internal Logic 6 to 9): a new version, a new file, a new folder and a removal. Each write is
//! one external-action bracket begun here before anything is sent and confirmed here after Graph accepts
//! it; none is resent automatically after an unknown outcome (G8).

use bytes::Bytes;
use ordin_journal::NewJournalEntry;
use reqwest::Method;

use crate::adapter::{
    valid_name, SharePointAdapter, MAX_FILE_BYTES, SIMPLE_UPLOAD_MAX, UPLOAD_RANGE,
};
use crate::error::SharePointError;
use crate::graph::{Failure, Req};
use crate::store;
use crate::types::{ExternalWrite, SharePointItemRef, SharePointLocation, SharePointVersion};
use crate::wire::{DriveItem, UploadSession};

/// What a content upload came to.
enum Uploaded {
    Done(Box<DriveItem>),
    /// Graph answered 412: the `If-Match` tag no longer matches.
    Precondition,
}

impl SharePointAdapter {
    /// Internal Logic 6: refused with `SP_ORIGIN_CHANGED` unless the item's cTag is `expected_version`.
    // spec: ordin-desktop/components/SharePointAdapter#C-69
    // spec: ordin-desktop/components/SharePointAdapter#SA-RECON-02
    pub async fn upload_version(
        &self,
        item: &SharePointItemRef,
        bytes: Vec<u8>,
        expected_version: &str,
        action: ExternalWrite,
    ) -> Result<SharePointVersion, SharePointError> {
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(SharePointError::TooLarge);
        }
        let size = bytes.len();
        let meta = self.item_meta(&item.drive_id, Some(&item.item_id)).await?;
        let current = meta.version(None)?;
        if current.tag != expected_version {
            return Err(SharePointError::OriginChanged { current });
        }
        let location = self.location_of(&meta, &item.drive_id).await?;
        self.begin(&action, Self::destination(&location)).await?;

        let body = Bytes::from(bytes);
        let mut etag = current.etag;
        let mut retried = false;
        let uploaded = loop {
            let attempt = self
                .put_content(
                    self.graph.url(
                        &["drives", &item.drive_id, "items", &item.item_id, "content"],
                        &[],
                    ),
                    self.graph.url(
                        &[
                            "drives",
                            &item.drive_id,
                            "items",
                            &item.item_id,
                            "createUploadSession",
                        ],
                        &[],
                    ),
                    &body,
                    Some(&etag),
                    "replace",
                )
                .await;
            match attempt {
                Ok(Uploaded::Done(done)) => break done,
                Ok(Uploaded::Precondition) => {
                    // The external action stays `sent`; the origin was not overwritten.
                    let current = self.item_meta(&item.drive_id, Some(&item.item_id)).await?;
                    return Err(SharePointError::OriginChanged {
                        current: current.version(None)?,
                    });
                }
                Err(Failure::Error(e)) => return Err(e),
                Err(Failure::Lost) => {
                    // Step 6.6: the request may or may not have landed. Read the item once.
                    let after = self.item_meta(&item.drive_id, Some(&item.item_id)).await;
                    match after {
                        Ok(after)
                            if after.ctag.as_deref() == Some(expected_version) && !retried =>
                        {
                            retried = true;
                            etag = after.etag.clone().ok_or(SharePointError::ResponseInvalid)?;
                        }
                        _ => return Err(self.outcome_unknown(&action)),
                    }
                }
            }
        };

        // Step 6.5: the new version, preferring a fresh read and falling back to Graph's own answer.
        let version = match self.version_of(item).await {
            Ok((_, version)) => version,
            Err(_) => uploaded.version(None)?,
        };
        self.settle(&action).await;
        // The next check must not report ordin's own push as a change (best effort: if this fails the
        // push is reported once).
        let _ = store::update_item_tags(&self.pool, &item.item_id, &version).await;
        self.record(&action, &version).await;
        tracing::info!(
            component = "sharepoint",
            kind = "version",
            size_bytes = size as u64,
            "sharepoint_pushed"
        );
        Ok(version)
    }

    /// Internal Logic 7.
    pub async fn create_file(
        &self,
        folder: &SharePointLocation,
        name: &str,
        bytes: Vec<u8>,
        action: ExternalWrite,
    ) -> Result<(SharePointItemRef, SharePointVersion), SharePointError> {
        if !valid_name(name) {
            return Err(SharePointError::LocationInvalid);
        }
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(SharePointError::TooLarge);
        }
        if !folder.is_folder {
            return Err(SharePointError::LocationInvalid);
        }
        let size = bytes.len();
        self.begin(&action, Self::destination(folder)).await?;

        let mut named = Self::parent_segments(folder);
        named.push(format!("{name}:"));
        let simple = self.child_url(
            &named,
            "content",
            &[("@microsoft.graph.conflictBehavior", "fail")],
        );
        let session = self.child_url(&named, "createUploadSession", &[]);
        let body = Bytes::from(bytes);
        let mut retried = false;
        let created = loop {
            match self
                .put_content(simple.clone(), session.clone(), &body, None, "fail")
                .await
            {
                Ok(Uploaded::Done(done)) => break done,
                Ok(Uploaded::Precondition) => return Err(SharePointError::ResponseInvalid),
                Err(Failure::Error(e)) => return Err(e),
                Err(Failure::Lost) => {
                    // Step 7.5: absent -> retry once; present -> the Push Service decides.
                    match self.exists(folder, name).await {
                        Ok(false) if !retried => retried = true,
                        _ => return Err(self.outcome_unknown(&action)),
                    }
                }
            }
        };
        let label = self
            .latest_label(&folder.drive_id, &created.id)
            .await
            .unwrap_or(None);
        let version = created.version(label)?;
        self.settle(&action).await;
        tracing::info!(
            component = "sharepoint",
            kind = "create",
            size_bytes = size as u64,
            "sharepoint_pushed"
        );
        Ok((
            SharePointItemRef {
                drive_id: folder.drive_id.clone(),
                item_id: created.id,
            },
            version,
        ))
    }

    /// Internal Logic 8.1.
    pub async fn create_folder(
        &self,
        parent: &SharePointLocation,
        name: &str,
        action: ExternalWrite,
    ) -> Result<SharePointLocation, SharePointError> {
        if !parent.is_folder || !valid_name(name) {
            return Err(SharePointError::LocationInvalid);
        }
        self.begin(&action, Self::destination(parent)).await?;
        let url = match &parent.item_id {
            Some(id) => self
                .graph
                .url(&["drives", &parent.drive_id, "items", id, "children"], &[]),
            None => self
                .graph
                .url(&["drives", &parent.drive_id, "root", "children"], &[]),
        };
        let body = serde_json::json!({
            "name": name,
            "folder": {},
            "@microsoft.graph.conflictBehavior": "fail",
        });
        let mut retried = false;
        let created: DriveItem = loop {
            match self
                .graph
                .send(
                    Req::new("create_folder", Method::POST, url.clone())
                        .json(&body)
                        .write(),
                )
                .await
            {
                Ok(reply) => break reply.json()?,
                Err(Failure::Error(e)) => return Err(e),
                Err(Failure::Lost) => match self.exists(parent, name).await {
                    Ok(false) if !retried => retried = true,
                    _ => return Err(self.outcome_unknown(&action)),
                },
            }
        };
        self.settle(&action).await;
        Ok(SharePointLocation {
            site_id: parent.site_id.clone(),
            site_name: parent.site_name.clone(),
            drive_id: parent.drive_id.clone(),
            drive_name: parent.drive_name.clone(),
            item_id: Some(created.id.clone()),
            path: Self::child_path(parent, name),
            is_folder: true,
            web_url: created.web_url.unwrap_or_default(),
        })
    }

    /// Internal Logic 8.3: moves the item to the recycle bin; nothing is purged.
    pub async fn delete(
        &self,
        item: &SharePointItemRef,
        expected_version: Option<&str>,
        action: ExternalWrite,
    ) -> Result<(), SharePointError> {
        let meta = self.item_meta(&item.drive_id, Some(&item.item_id)).await?;
        let current = meta.version(None)?;
        if expected_version.is_some_and(|expected| expected != current.tag) {
            return Err(SharePointError::OriginChanged { current });
        }
        let location = self.location_of(&meta, &item.drive_id).await?;
        self.begin(&action, Self::destination(&location)).await?;
        let url = self
            .graph
            .url(&["drives", &item.drive_id, "items", &item.item_id], &[]);
        let mut etag = current.etag;
        let mut retried = false;
        loop {
            let sent = self
                .graph
                .send(
                    Req::new("delete_item", Method::DELETE, url.clone())
                        .header("if-match", etag.clone())
                        .write()
                        .pass(&[412]),
                )
                .await;
            match sent {
                Ok(reply) if reply.status == 412 => {
                    let current = self.item_meta(&item.drive_id, Some(&item.item_id)).await?;
                    return Err(SharePointError::OriginChanged {
                        current: current.version(None)?,
                    });
                }
                Ok(_) => break,
                Err(Failure::Error(e)) => return Err(e),
                Err(Failure::Lost) => {
                    match self.item_meta(&item.drive_id, Some(&item.item_id)).await {
                        // Gone: the removal landed.
                        Err(SharePointError::NotFound) => break,
                        Ok(after)
                            if after.ctag.as_deref() == Some(current.tag.as_str()) && !retried =>
                        {
                            retried = true;
                            etag = after.etag.clone().ok_or(SharePointError::ResponseInvalid)?;
                        }
                        _ => return Err(self.outcome_unknown(&action)),
                    }
                }
            }
        }
        self.settle(&action).await;
        // The next check must not report ordin's own removal (best effort).
        let _ = store::delete_item_rows(&self.pool, &item.item_id).await;
        Ok(())
    }

    /// `…/{parent}:/{name}:/<tail>` with the given query.
    fn child_url(&self, named: &[String], tail: &str, query: &[(&str, &str)]) -> String {
        let mut parts: Vec<&str> = named.iter().map(String::as_str).collect();
        parts.push(tail);
        self.graph.url(&parts, query)
    }

    /// Uploads `body` by one `PUT` up to 4 MiB, else by an upload session of 10 MiB ranges. `if_match` is
    /// the eTag just read for a replacement.
    async fn put_content(
        &self,
        simple_url: String,
        session_url: String,
        body: &Bytes,
        if_match: Option<&str>,
        conflict: &str,
    ) -> Result<Uploaded, Failure> {
        if body.len() <= SIMPLE_UPLOAD_MAX {
            let mut request = Req::new("upload_content", Method::PUT, simple_url)
                .bytes(body.clone())
                .transfer()
                .write()
                .pass(&[412]);
            if let Some(tag) = if_match {
                request = request.header("if-match", tag);
            }
            let reply = self.graph.send(request).await?;
            if reply.status == 412 {
                return Ok(Uploaded::Precondition);
            }
            return Ok(Uploaded::Done(Box::new(reply.json()?)));
        }
        let mut open = Req::new("upload_session", Method::POST, session_url)
            .json(&serde_json::json!({
                "item": {"@microsoft.graph.conflictBehavior": conflict}
            }))
            .write()
            .pass(&[412]);
        if let Some(tag) = if_match {
            open = open.header("if-match", tag);
        }
        let opened = self.graph.send(open).await?;
        if opened.status == 412 {
            return Ok(Uploaded::Precondition);
        }
        let session: UploadSession = opened.json()?;
        let total = body.len();
        let mut start = 0;
        while start < total {
            let end = (start + UPLOAD_RANGE).min(total);
            let range = Req::new("upload_range", Method::PUT, session.upload_url.clone())
                .bytes(body.slice(start..end))
                .header(
                    "content-range",
                    format!("bytes {start}-{}/{total}", end - 1),
                )
                .transfer()
                .write()
                .anonymous();
            let reply = self.graph.send(range).await?;
            if end == total {
                return Ok(Uploaded::Done(Box::new(reply.json()?)));
            }
            start = end;
        }
        Err(SharePointError::ResponseInvalid.into())
    }

    /// Logs `sharepoint_outcome_unknown`; the external action stays `sent` for the Push Service.
    fn outcome_unknown(&self, action: &ExternalWrite) -> SharePointError {
        tracing::warn!(
            component = "sharepoint",
            action_key = action.key.to_string().as_str(),
            "sharepoint_outcome_unknown"
        );
        SharePointError::OutcomeUnknown
    }

    /// Confirms the external action. A failure after Graph accepted the write is logged and does not fail
    /// the write: the Journal's start-up recovery settles a `sent` row.
    async fn settle(&self, action: &ExternalWrite) {
        if self
            .journal
            .confirm_external_action(action.key)
            .await
            .is_err()
        {
            tracing::warn!(
                component = "sharepoint",
                action_key = action.key.to_string().as_str(),
                "sharepoint_confirm_failed"
            );
        }
    }

    /// The Journal entry of a pushed version: ids and the version label only, no name and no text.
    async fn record(&self, action: &ExternalWrite, version: &SharePointVersion) {
        let kind = serde_json::to_value(action.kind).unwrap_or(serde_json::Value::Null);
        let appended = self
            .journal
            .append_entry(NewJournalEntry {
                kind: "sharepoint_pushed".to_owned(),
                component: "sharepoint".to_owned(),
                run_id: action.run_id,
                crew_run_id: None,
                workspace_id: None,
                summary: "A new version was sent to SharePoint.".to_owned(),
                data: serde_json::json!({"kind": kind, "version_label": version.label}),
            })
            .await;
        if appended.is_err() {
            tracing::warn!(
                component = "sharepoint",
                action_key = action.key.to_string().as_str(),
                "sharepoint_confirm_failed"
            );
        }
    }
}
