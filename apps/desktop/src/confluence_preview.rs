//! DSK-CFP-1: the preview of a linked Confluence page. A page is kept in its working copy as
//! `<title>.confluence.xhtml`, Confluence's storage format (ConfluenceAdapter.md, *Working-copy file
//! names*), which the File Service has no preview for; the program converts it with the Confluence
//! Adapter's `storage_to_markdown` and answers it as Markdown text, cut as any text preview is.
// spec: ordin-desktop/components/OrdinDesktop#DSK-CFP-1

use ordin_confluence::ConfluenceAdapter;
use ordin_files::{Preview, PREVIEW_TEXT_BYTES};

const PAGE_SUFFIX: &str = ".confluence.xhtml";

/// Whether a working-copy path names a Confluence page.
pub(crate) fn is_page(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(PAGE_SUFFIX)
}

/// The page's Markdown; a page that does not convert has no preview.
pub(crate) fn preview(bytes: &[u8]) -> Preview {
    let storage = String::from_utf8_lossy(bytes);
    match ConfluenceAdapter::storage_to_markdown(&storage) {
        Ok(markdown) => {
            let truncated = markdown.len() > PREVIEW_TEXT_BYTES;
            let mut end = markdown.len().min(PREVIEW_TEXT_BYTES);
            while !markdown.is_char_boundary(end) {
                end -= 1;
            }
            Preview::Text {
                text: markdown[..end].to_owned(),
                markdown: true,
                truncated,
            }
        }
        Err(_) => Preview::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_reads_as_markdown() {
        assert!(is_page("Supplier pages/Rates.confluence.xhtml"));
        assert!(!is_page("Rates.xhtml"));
        match preview(b"<h1>Rates</h1><p>Kestrel Freight: 1,240 GBP.</p>") {
            Preview::Text {
                text,
                markdown,
                truncated,
            } => {
                assert!(markdown && !truncated);
                assert!(text.contains("# Rates"), "{text}");
                assert!(text.contains("Kestrel Freight: 1,240 GBP."), "{text}");
            }
            other => panic!("not text: {other:?}"),
        }
    }

    #[test]
    fn a_page_that_does_not_convert_has_no_preview() {
        assert!(matches!(preview(b"<p>unclosed"), Preview::None));
    }
}
