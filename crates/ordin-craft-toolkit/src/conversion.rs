//! Published copies and the reverse conversion (CraftToolkit.md *Internal Logic* 7 and 9).

use std::path::{Path, PathBuf};
use std::time::Instant;

use ordin_rules::PathAccessRequest;
use ordin_types::domain::engineering::CraftInstallationStatus;
use ordin_types::domain::places::{CopyFormat, CopyMaker, FolderAccess};
use ordin_types::time::Timestamp;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use serde_json::{json, Map, Value};

use crate::definitions::{CraftToolDefinition, PathRole};
use crate::error::CraftError;
use crate::model::{MadeCopy, MakeCopy};
use crate::servers::Owner;
use crate::toolkit::CraftToolkit;

const WORD_APP: &str = "wordcraft";

/// The maker of a copy format: its app, its `CopyMaker` and the file extension.
fn maker_of(format: CopyFormat) -> Option<(&'static str, CopyMaker, &'static str)> {
    match format {
        CopyFormat::Word => Some((WORD_APP, CopyMaker::WordCraft, "docx")),
        CopyFormat::Pdf => Some(("pdfcraft", CopyMaker::PdfCraft, "pdf")),
        CopyFormat::Slides => Some(("deckcraft", CopyMaker::DeckCraft, "pptx")),
        CopyFormat::Page | CopyFormat::ExactFile => None,
    }
}

/// `BANNER_TEMPLATE` (ADR-069).
// 7.2: neither `MakeCopy` nor `CraftDeps` carries a locale and this crate has no path to the
// Localisation module, so the date is written in en-GB, in UTC (a recorded exception to SA-I18N-02).
// spec: ordin-desktop/components/CraftToolkit#ADR-069
pub(crate) fn banner(
    original: &str,
    from: &str,
    branch: Option<&str>,
    updated_at: &Timestamp,
) -> String {
    let branch = branch.map_or_else(String::new, |b| format!(", branch {b}"));
    let date = updated_at.0.format("%-d %B %Y, %H:%M");
    format!(
        "Published copy of {original}, from {from}{branch}. Last updated {date}. \
         Make changes in the original."
    )
}

/// Slides with no footer argument: the banner ends the first slide's content, the text under the
/// first heading (*assumed here until C-28 shows DeckCraft's arguments*).
pub(crate) fn banner_in_first_slide(markdown: &str, banner: &str) -> String {
    let is_heading = |line: &str| line.starts_with('#');
    let lines: Vec<&str> = markdown.lines().collect();
    let first = lines.iter().position(|line| is_heading(line));
    let end = first.map_or(lines.len(), |first| {
        lines
            .iter()
            .enumerate()
            .skip(first + 1)
            .find(|(_, line)| is_heading(line) || line.trim() == "---")
            .map_or(lines.len(), |(at, _)| at)
    });
    let mut head = lines[..end].join("\n").trim_end().to_owned();
    head.push_str("\n\n");
    head.push_str(banner);
    head.push('\n');
    if end < lines.len() {
        head.push('\n');
        head.push_str(&lines[end..].join("\n"));
        head.push('\n');
    }
    head
}

/// The arguments naming the tool's first input and output.
fn path_arg(tool: &CraftToolDefinition, role: PathRole) -> Option<&str> {
    tool.path_args
        .iter()
        .find(|arg| arg.role == role)
        .map(|arg| arg.name.as_str())
}

impl CraftToolkit {
    /// A tool of the installed `app` with MCP mode that `pick` selects, among the tools the profile
    /// in force allows: a withdrawn app, or a tool `allowed_tools` removed, is never sent a document,
    /// even while its files wait on disk for the next `sync_installations`.
    fn conversion_tool(
        &self,
        app: &str,
        pick: impl Fn(&CraftToolDefinition) -> bool,
    ) -> Option<CraftToolDefinition> {
        let row = self.row(app)?;
        if row.status != CraftInstallationStatus::Installed || !row.has_mcp() {
            return None;
        }
        let definition = self.installed_definition(&row)?;
        Self::allowed_tools(&self.craft_profile(), app, definition)
            .into_iter()
            .find(|tool| pick(tool))
            .cloned()
    }

    fn copy_tool(&self, format: CopyFormat) -> Option<CraftToolDefinition> {
        let (app, _, _) = maker_of(format)?;
        self.conversion_tool(app, |tool| tool.converts_markdown_to == Some(format))
    }

    // spec: ordin-desktop/components/CraftToolkit#C-28
    pub fn conversion_available(&self, format: CopyFormat) -> bool {
        self.copy_tool(format).is_some()
    }

    /// Internal Logic 7.3.
    fn check_conversion_path(&self, path: &Path, access: FolderAccess) -> Result<(), CraftError> {
        self.path_access(&PathAccessRequest {
            workspace_id: None,
            agent: None,
            crew_charter_id: None,
            path: path.to_path_buf(),
            access,
        })
    }

    /// Writes `input`, runs the tool in the shared conversion session and reads `output`. Both
    /// scratch files are gone afterwards, whatever the outcome.
    async fn convert(
        &self,
        app: &str,
        tool: &CraftToolDefinition,
        input: &Path,
        input_bytes: &[u8],
        output: &Path,
        extra: Map<String, Value>,
    ) -> Result<Vec<u8>, CraftError> {
        let input_arg = path_arg(tool, PathRole::Input).ok_or(CraftError::ConversionUnavailable)?;
        let output_arg =
            path_arg(tool, PathRole::Output).ok_or(CraftError::ConversionUnavailable)?;
        self.check_conversion_path(input, FolderAccess::ReadAndChange)?;
        self.check_conversion_path(output, FolderAccess::ReadAndChange)?;
        let mut arguments = extra;
        arguments.insert(input_arg.to_owned(), json!(input.to_string_lossy()));
        arguments.insert(output_arg.to_owned(), json!(output.to_string_lossy()));
        let result = self
            .run_conversion(app, tool, input, input_bytes, output, arguments)
            .await;
        let _ = tokio::fs::remove_file(input).await;
        let _ = tokio::fs::remove_file(output).await;
        result
    }

    async fn run_conversion(
        &self,
        app: &str,
        tool: &CraftToolDefinition,
        input: &Path,
        input_bytes: &[u8],
        output: &Path,
        arguments: Map<String, Value>,
    ) -> Result<Vec<u8>, CraftError> {
        tokio::fs::create_dir_all(self.conversions_folder())
            .await
            .map_err(|_| CraftError::ToolFailed)?;
        tokio::fs::write(input, input_bytes)
            .await
            .map_err(|_| CraftError::ToolFailed)?;
        let handle = self.server(Owner::Conversion, app).await?;
        if !handle.lists(&tool.tool) {
            return Err(CraftError::ConversionUnavailable);
        }
        let params = CallToolRequestParams::new(tool.tool.clone()).with_arguments(arguments);
        let called = tokio::time::timeout(self.limits.call, handle.peer.call_tool(params)).await;
        drop(handle);
        let result: CallToolResult = match called {
            Err(_) => {
                self.kill_server(Owner::Conversion, app, "timeout").await;
                return Err(CraftError::Timeout);
            }
            Ok(Err(_)) => return Err(CraftError::ToolFailed),
            Ok(Ok(result)) => result,
        };
        if result.is_error == Some(true) {
            return Err(CraftError::ToolFailed);
        }
        self.read_output(output).await
    }

    fn scratch_pair(&self, input_extension: &str, output_extension: &str) -> (PathBuf, PathBuf) {
        let id = uuid::Uuid::new_v4();
        let folder = self.conversions_folder();
        (
            folder.join(format!("{id}.{input_extension}")),
            folder.join(format!("{id}.{output_extension}")),
        )
    }

    // spec: ordin-desktop/components/CraftToolkit#ADR-048
    pub async fn make_copy(&self, req: MakeCopy) -> Result<MadeCopy, CraftError> {
        let started = Instant::now();
        let (app, maker, extension) =
            maker_of(req.format).ok_or(CraftError::ConversionUnavailable)?;
        let tool = self
            .copy_tool(req.format)
            .ok_or(CraftError::ConversionUnavailable)?;
        let line = banner(
            &req.original_name,
            &req.original_from,
            req.branch.as_deref(),
            &req.updated_at,
        );
        let mut extra = Map::new();
        let markdown = match (req.format, &tool.footer_arg) {
            (CopyFormat::Slides, Some(footer)) => {
                extra.insert(footer.clone(), json!(line));
                req.markdown.clone()
            }
            (CopyFormat::Slides, None) => banner_in_first_slide(&req.markdown, &line),
            _ => format!("{line}\n\n{}", req.markdown),
        };
        let (input, output) = self.scratch_pair("md", extension);
        let bytes = self
            .convert(app, &tool, &input, markdown.as_bytes(), &output, extra)
            .await?;
        let stem = Path::new(&req.original_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&req.original_name);
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let format = format!("{:?}", req.format).to_lowercase();
        tracing::info!(
            component = "craft",
            format = format.as_str(),
            elapsed_ms,
            "craft_copy_made"
        );
        self.record(
            "craft_copy_made",
            None,
            "A published copy was made.".to_owned(),
            json!({ "format": format, "elapsed_ms": elapsed_ms }),
        )
        .await;
        Ok(MadeCopy {
            bytes,
            file_name: format!("{stem}.{extension}"),
            maker,
        })
    }

    /// Reverse conversion, conditional on C-13 (SA-RECON-03). `format` must be `Word`.
    // spec: ordin-desktop/components/CraftToolkit#SA-RECON-03
    // spec: ordin-desktop/components/CraftToolkit#C-13
    pub async fn to_markdown(
        &self,
        format: CopyFormat,
        bytes: Vec<u8>,
    ) -> Result<String, CraftError> {
        let started = Instant::now();
        if format != CopyFormat::Word {
            return Err(CraftError::ConversionUnavailable);
        }
        let tool = self
            .conversion_tool(WORD_APP, |tool| {
                tool.converts_to_markdown_from == Some(CopyFormat::Word)
            })
            .ok_or(CraftError::ConversionUnavailable)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > self.limits.max_output {
            return Err(CraftError::OutputTooLarge);
        }
        let (input, output) = self.scratch_pair("docx", "md");
        let made = self
            .convert(WORD_APP, &tool, &input, &bytes, &output, Map::new())
            .await?;
        let markdown = String::from_utf8(made).map_err(|_| CraftError::ToolFailed)?;
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        tracing::info!(component = "craft", elapsed_ms, "craft_markdown_made");
        self.record(
            "craft_markdown_made",
            None,
            "A Word file was turned into Markdown.".to_owned(),
            json!({ "elapsed_ms": elapsed_ms }),
        )
        .await;
        Ok(markdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> Timestamp {
        Timestamp(ordin_types::time::parse_rfc3339_ms_utc("2026-03-14T09:12:00.000Z").unwrap())
    }

    #[test]
    fn the_banner_leaves_out_the_branch_clause_when_there_is_no_branch() {
        assert_eq!(
            banner("rates.md", "supplier-docs", Some("main"), &at()),
            "Published copy of rates.md, from supplier-docs, branch main. Last updated \
             14 March 2026, 09:12. Make changes in the original."
        );
        assert_eq!(
            banner("rates.md", "Brand", None, &at()),
            "Published copy of rates.md, from Brand. Last updated 14 March 2026, 09:12. \
             Make changes in the original."
        );
    }

    #[test]
    fn the_banner_ends_the_first_slide() {
        let deck = "# One\n\nText\n\n# Two\n\nMore\n";
        assert_eq!(
            banner_in_first_slide(deck, "B"),
            "# One\n\nText\n\nB\n\n# Two\n\nMore\n"
        );
        let ruled = "# One\n---\n# Two\n";
        assert_eq!(
            banner_in_first_slide(ruled, "B"),
            "# One\n\nB\n\n---\n# Two\n"
        );
        assert_eq!(banner_in_first_slide("# Only", "B"), "# Only\n\nB\n");
        assert_eq!(
            banner_in_first_slide("no heading", "B"),
            "no heading\n\nB\n"
        );
    }
}
