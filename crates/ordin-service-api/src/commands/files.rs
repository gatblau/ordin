//! Documents — restated from `FileService.md` (SA-RECON-02).
//!
//! Places, folder listings, previews, versions, the bin, search and downloads. `Place`, `FileVersion`,
//! `PublishedCopy` and `CopyOriginal` are 2C types and are imported. The rows `files.save_copy` and
//! `files.save_export` are the desktop program's and take this spec's root types.

use ordin_types::prelude::*;
use ordin_types::text::{check_abs_path, AreaCodes};
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{CopyOriginal, FileVersion, Place, PublishedCopy};

/// `Place.friendly_name`: 1–120 characters (2C, `PLACE_NAME_INVALID`).
const PLACE_NAME_MAX_CHARS: usize = 120;
/// A path relative to its place: at most 4,096 bytes (2C `FileVersion.path`, `FVER_PATH_INVALID`).
const PLACE_PATH_MAX_BYTES: usize = 4096;
/// `files.search`: `query` 1–200 characters, `limit` 1–200.
const QUERY_MAX_CHARS: usize = 200;
const SEARCH_LIMIT_MAX: u32 = 200;
/// `files.save_edit.text`: at most 8 MiB of UTF-8 (ServiceApi.md *Input limits*).
const SAVE_EDIT_TEXT_MAX_BYTES: usize = 8 * 1024 * 1024;

/// The absolute-path rule (`PLACE_PATH_INVALID`), refused here as `SVC_INPUT_INVALID`.
const PATH_CODES: AreaCodes = AreaCodes {
    name_empty: rules::INPUT_INVALID,
    name_too_long: rules::INPUT_INVALID,
    path_invalid: rules::INPUT_INVALID,
    url_invalid: rules::INPUT_INVALID,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FolderSort {
    NameAsc,
    NameDesc,
    KindAsc,
    ChangedDesc,
    ChangedAsc,
    ChangedByAsc,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FileEntry {
    pub place_id: PlaceId,
    /// Relative to the place, `/`-separated; a folder ends with `/`.
    pub path: String,
    pub name: String,
    pub is_folder: bool,
    /// Lower-case extension without the dot; empty for a folder or no extension.
    pub kind: String,
    pub size_bytes: u64,
    pub changed_at: Timestamp,
    /// "Ordin", an agent instance's display name supplied by the writer, "you", or the origin's author
    /// recorded by `set_attribution`; absent when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
    pub has_versions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Preview {
    /// The WebView draws it from the bytes (png, jpeg, gif, webp, svg as an image, never as markup).
    Image {
        media_type: String,
        bytes_b64: String,
    },
    /// The WebView's own PDF viewer draws it.
    Pdf { bytes_b64: String },
    /// Plain text or Markdown, the first `PREVIEW_TEXT_BYTES`; `truncated` when longer.
    Text {
        text: String,
        markdown: bool,
        truncated: bool,
    },
    /// A platform thumbnail (macOS Quick Look); PNG.
    Thumbnail { png_b64: String },
    /// No preview for this kind on this platform; the screens show the kind's icon.
    None,
}

/// Undo record of one `move_to_bin`; the bytes are the version `file_version_id` in the version store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BinTicket {
    pub place_id: PlaceId,
    pub path: String,
    pub file_version_id: FileVersionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub coverage: IndexCoverage,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SearchHit {
    pub place_id: PlaceId,
    pub path: String,
    pub name_match: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    pub changed_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct IndexCoverage {
    pub indexed: u64,
    pub total: u64,
}

// ---- inputs ----

/// `files.add_place`: the path comes from the system folder picker.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FilesAddPlaceInput {
    pub path: String,
    pub friendly_name: Option<String>,
}

/// `files.rename_place`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FilesRenamePlaceInput {
    pub place_id: PlaceId,
    pub friendly_name: String,
}

/// `files.list_folder`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FilesListFolderInput {
    pub place_id: PlaceId,
    pub folder: String,
    pub sort: FolderSort,
    pub cursor: Option<Cursor>,
}

/// Declares the inputs `{ place_id, path }`.
macro_rules! place_path_inputs {
    ($($(#[$doc:meta])* $name:ident;)*) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(rename_all = "snake_case", deny_unknown_fields)]
        pub struct $name {
            pub place_id: PlaceId,
            pub path: String,
        }

        impl CommandInput for $name {
            fn check(&mut self) -> Result<(), InputRejection> {
                place_path(&self.path, 1, "/path")
            }
        }
    )*};
}

place_path_inputs! {
    /// `files.preview`.
    FilesPreviewInput;
    /// `files.versions`.
    FilesVersionsInput;
    /// `files.move_to_bin`.
    FilesMoveToBinInput;
    /// `files.download`.
    FilesDownloadInput;
}

/// `files.restore_version`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FilesRestoreVersionInput {
    pub file_version_id: FileVersionId,
}

/// `files.import`: the sources are paths from a drop event.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FilesImportInput {
    pub place_id: PlaceId,
    pub folder: String,
    pub sources: Vec<String>,
}

/// `files.restore_from_bin`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FilesRestoreFromBinInput {
    pub ticket: BinTicket,
}

/// `files.save_edit` (9 MiB body).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FilesSaveEditInput {
    pub place_id: PlaceId,
    pub path: String,
    pub text: String,
}

/// `files.published_copies`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FilesPublishedCopiesInput {
    pub original: CopyOriginal,
}

/// `files.search`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FilesSearchInput {
    pub place_ids: Vec<PlaceId>,
    pub query: String,
    pub limit: u32,
}

/// `files.download`, over `read_file`; the desktop's **Save a copy** writes it.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FilesDownloadResult {
    pub name: String,
    pub media_type: String,
    pub bytes_b64: String,
}

/// A path relative to its place: `min`–4,096 bytes, no leading `/`, no `..` segment, no NUL.
fn place_path(value: &str, min: usize, at: &str) -> Result<(), InputRejection> {
    rules::bytes(value, min, PLACE_PATH_MAX_BYTES, at)?;
    if value.starts_with('/')
        || value.contains('\0')
        || value.split(['/', '\\']).any(|segment| segment == "..")
    {
        return Err(rules::reject(at));
    }
    Ok(())
}

impl CommandInput for FilesAddPlaceInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_abs_path(&self.path, PATH_CODES).map_err(|_| rules::reject("/path"))?;
        rules::opt_text(
            &self.friendly_name,
            1,
            PLACE_NAME_MAX_CHARS,
            "/friendly_name",
        )
    }
}

impl CommandInput for FilesRenamePlaceInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(
            &self.friendly_name,
            1,
            PLACE_NAME_MAX_CHARS,
            "/friendly_name",
        )
    }
}

impl CommandInput for FilesListFolderInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        place_path(&self.folder, 0, "/folder")?;
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for FilesRestoreVersionInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for FilesImportInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        place_path(&self.folder, 0, "/folder")?;
        for (i, source) in self.sources.iter().enumerate() {
            check_abs_path(source, PATH_CODES)
                .map_err(|_| rules::reject(&format!("/sources/{i}")))?;
        }
        Ok(())
    }
}

impl CommandInput for FilesRestoreFromBinInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        place_path(&self.ticket.path, 1, "/ticket/path")
    }
}

impl CommandInput for FilesSaveEditInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        place_path(&self.path, 1, "/path")?;
        rules::bytes(&self.text, 0, SAVE_EDIT_TEXT_MAX_BYTES, "/text")
    }
}

impl CommandInput for FilesPublishedCopiesInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        match &self.original {
            CopyOriginal::PlaceFile { path, .. } => place_path(path, 1, "/original/path"),
            CopyOriginal::KnowledgePage { .. } => Ok(()),
        }
    }
}

impl CommandInput for FilesSearchInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.query, 1, QUERY_MAX_CHARS, "/query")?;
        rules::range(self.limit, 1, SEARCH_LIMIT_MAX, "/limit")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused_at(result: Result<(), InputRejection>) -> String {
        let err = result.unwrap_err();
        assert_eq!(err.code, "SVC_INPUT_INVALID");
        err.at
    }

    #[test]
    fn place_paths() {
        assert!(place_path("Q3/Kestrel quote.pdf", 1, "/p").is_ok());
        assert!(place_path("", 0, "/p").is_ok());
        for bad in ["", "/abs", "a/../b", "a\0b", &"x".repeat(4097)] {
            assert_eq!(refused_at(place_path(bad, 1, "/p")), "/input/p");
        }
        let place_id = PlaceId::new();
        assert!(FilesPreviewInput {
            place_id,
            path: "a.md".into()
        }
        .check()
        .is_ok());
        assert_eq!(
            refused_at(
                FilesVersionsInput {
                    place_id,
                    path: "../a".into()
                }
                .check()
            ),
            "/input/path"
        );
        assert!(FilesMoveToBinInput {
            place_id,
            path: "a".into()
        }
        .check()
        .is_ok());
        assert!(FilesDownloadInput {
            place_id,
            path: "a".into()
        }
        .check()
        .is_ok());
    }

    #[test]
    fn places() {
        let mut add = FilesAddPlaceInput {
            path: "/Users/asha/Documents/Supplier quotes".into(),
            friendly_name: None,
        };
        assert!(add.check().is_ok());
        add.friendly_name = Some("x".repeat(121));
        assert_eq!(refused_at(add.check()), "/input/friendly_name");
        let mut relative = FilesAddPlaceInput {
            path: "Documents".into(),
            friendly_name: None,
        };
        assert_eq!(refused_at(relative.check()), "/input/path");
        let mut rename = FilesRenamePlaceInput {
            place_id: PlaceId::new(),
            friendly_name: String::new(),
        };
        assert_eq!(refused_at(rename.check()), "/input/friendly_name");
        rename.friendly_name = "Quotes".into();
        assert!(rename.check().is_ok());
    }

    #[test]
    fn listing_import_and_bin() {
        let place_id = PlaceId::new();
        let mut list = FilesListFolderInput {
            place_id,
            folder: "Q3/".into(),
            sort: FolderSort::ChangedDesc,
            cursor: None,
        };
        assert!(list.check().is_ok());
        list.cursor = Some(Cursor::encode(&"x".repeat(900)));
        assert_eq!(list.check().unwrap_err().code, "SRV_BAD_CURSOR");
        list.folder = "/Q3".into();
        assert_eq!(refused_at(list.check()), "/input/folder");

        let mut import = FilesImportInput {
            place_id,
            folder: String::new(),
            sources: vec!["/Users/asha/Downloads/a.pdf".into(), "b.pdf".into()],
        };
        assert_eq!(refused_at(import.check()), "/input/sources/1");
        import.sources.pop();
        assert!(import.check().is_ok());
        import.folder = "../x".into();
        assert_eq!(refused_at(import.check()), "/input/folder");

        let mut restore = FilesRestoreFromBinInput {
            ticket: BinTicket {
                place_id,
                path: "a.md".into(),
                file_version_id: FileVersionId::new(),
            },
        };
        assert!(restore.check().is_ok());
        restore.ticket.path = String::new();
        assert_eq!(refused_at(restore.check()), "/input/ticket/path");
        assert!(FilesRestoreVersionInput {
            file_version_id: FileVersionId::new()
        }
        .check()
        .is_ok());
    }

    #[test]
    fn edits_copies_and_search() {
        let place_id = PlaceId::new();
        let mut edit = FilesSaveEditInput {
            place_id,
            path: "notes.md".into(),
            text: "# Notes".into(),
        };
        assert!(edit.check().is_ok());
        edit.text = "x".repeat(8 * 1024 * 1024 + 1);
        assert_eq!(refused_at(edit.check()), "/input/text");

        let mut copies = FilesPublishedCopiesInput {
            original: CopyOriginal::PlaceFile {
                place_id,
                path: "/abs".into(),
            },
        };
        assert_eq!(refused_at(copies.check()), "/input/original/path");
        copies.original = CopyOriginal::KnowledgePage {
            knowledge_page_id: KnowledgePageId::new(),
        };
        assert!(copies.check().is_ok());

        let search = |query: &str, limit| FilesSearchInput {
            place_ids: vec![place_id],
            query: query.into(),
            limit,
        };
        assert!(search("Kestrel day rate", 20).check().is_ok());
        assert_eq!(refused_at(search("", 20).check()), "/input/query");
        assert_eq!(refused_at(search("x", 0).check()), "/input/limit");
        assert_eq!(refused_at(search("x", 201).check()), "/input/limit");
    }

    #[test]
    fn list_folder_example_reads() {
        let input: FilesListFolderInput = serde_json::from_value(serde_json::json!({
            "place_id": "0192a0f2-2222-7333-8444-555566667777",
            "folder": "Q3/", "sort": "changed_desc", "cursor": null
        }))
        .unwrap();
        assert_eq!(input.sort, FolderSort::ChangedDesc);
        let preview = Preview::Text {
            text: "# Q3".into(),
            markdown: true,
            truncated: false,
        };
        assert_eq!(serde_json::to_value(&preview).unwrap()["kind"], "text");
    }
}
