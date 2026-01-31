//! Previews of the common kinds (FileService.md Internal Logic 4.5).

use std::time::Duration;

use base64::Engine as _;
use ordin_types::PlaceId;

use crate::error::FileError;
use crate::extract::{cut, extract_text};
use crate::paths::{
    self, is_markdown_kind, is_office_kind, is_office_zip_kind, is_text_kind, kind_of, name_of,
    normalise_file,
};
use crate::service::FileService;
use crate::types::Preview;
use crate::versions::blocking;
use crate::{INDEX_MAX_SOURCE_BYTES, PREVIEW_TEXT_BYTES};

const IMAGE_PREVIEW_LIMIT: u64 = 20 * 1024 * 1024;
const PDF_PREVIEW_LIMIT: u64 = 64 * 1024 * 1024;
const THUMBNAIL_LIMIT: Duration = Duration::from_secs(10);

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

impl FileService {
    pub async fn preview(&self, place_id: PlaceId, path: &str) -> Result<Preview, FileError> {
        let path = normalise_file(path)?;
        let place = self.place_row(place_id).await?;
        let abs = paths::confine(&place.root, &path)?;
        let kind = kind_of(name_of(&path));
        let probe = abs.clone();
        let size = blocking(move || {
            std::fs::metadata(&probe)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len())
        })
        .await?
        .ok_or(FileError::NotFound)?;
        if matches!(
            kind.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
        ) {
            if size > IMAGE_PREVIEW_LIMIT {
                return Ok(Preview::None);
            }
            let bytes = self.read_limited(&abs).await?.ok_or(FileError::NotFound)?;
            // An SVG is returned as image bytes; the WebView draws it in an `<img>`, never inline.
            return Ok(Preview::Image {
                media_type: paths::media_type(&kind).to_owned(),
                bytes_b64: b64(&bytes),
            });
        }
        if kind == "pdf" {
            if size > PDF_PREVIEW_LIMIT {
                return Ok(Preview::None);
            }
            let bytes = self.read_limited(&abs).await?.ok_or(FileError::NotFound)?;
            return Ok(Preview::Pdf {
                bytes_b64: b64(&bytes),
            });
        }
        if is_text_kind(&kind) {
            let target = abs.clone();
            let head = blocking(move || -> std::io::Result<Vec<u8>> {
                use std::io::Read;
                let mut buffer = Vec::new();
                std::fs::File::open(&target)?
                    .take(PREVIEW_TEXT_BYTES as u64)
                    .read_to_end(&mut buffer)?;
                Ok(buffer)
            })
            .await?
            .map_err(|_| FileError::NotFound)?;
            return Ok(Preview::Text {
                text: String::from_utf8_lossy(&head).into_owned(),
                markdown: is_markdown_kind(&kind),
                truncated: size > PREVIEW_TEXT_BYTES as u64,
            });
        }
        if is_office_kind(&kind) {
            if let Some(png) = thumbnail(&abs).await {
                return Ok(Preview::Thumbnail { png_b64: b64(&png) });
            }
            return office_text(abs, kind, size).await;
        }
        Ok(Preview::None)
    }
}

/// With no thumbnail, a `docx`, `xlsx` or `pptx` file's text as the index reads it, one paragraph a
/// line; any other office kind, or a file whose text cannot be read, has no preview (Internal Logic 4.5).
// spec: ordin-desktop/components/FileService#FS-PV-1
async fn office_text(
    abs: std::path::PathBuf,
    kind: String,
    size: u64,
) -> Result<Preview, FileError> {
    if !is_office_zip_kind(&kind) || size > INDEX_MAX_SOURCE_BYTES {
        return Ok(Preview::None);
    }
    let text = blocking(move || {
        std::fs::read(&abs)
            .ok()
            .and_then(|bytes| extract_text(&kind, &bytes).ok().flatten())
    })
    .await?;
    Ok(match text {
        Some(text) if !text.trim().is_empty() => {
            let truncated = text.len() > PREVIEW_TEXT_BYTES;
            Preview::Text {
                text: cut(text.trim_end().to_owned(), PREVIEW_TEXT_BYTES),
                markdown: false,
                truncated,
            }
        }
        _ => Preview::None,
    })
}

/// macOS Quick Look through `/usr/bin/qlmanage`, with a 10 s limit. Other systems, and any failure,
/// give no thumbnail (the screens show the kind's icon).
async fn thumbnail(file: &std::path::Path) -> Option<Vec<u8>> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = std::env::temp_dir().join(format!("ordin-thumb-{}", uuid::Uuid::now_v7()));
    tokio::fs::create_dir_all(&out).await.ok()?;
    let run = tokio::process::Command::new("/usr/bin/qlmanage")
        .arg("-t")
        .arg("-s")
        .arg("1024")
        .arg("-o")
        .arg(&out)
        .arg(file)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .status();
    let finished = tokio::time::timeout(THUMBNAIL_LIMIT, run).await;
    let png = match finished {
        Ok(Ok(status)) if status.success() => {
            let name = format!("{}.png", file.file_name()?.to_str()?);
            tokio::fs::read(out.join(name)).await.ok()
        }
        _ => None,
    };
    let _ = tokio::fs::remove_dir_all(&out).await;
    png
}
