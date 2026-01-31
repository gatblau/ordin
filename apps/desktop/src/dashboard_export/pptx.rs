//! The PowerPoint presentation (DSK-DBX-1) and new slides added to an existing one (DSK-DBX-2).
//!
//! A title slide with the dashboard's name and the as-of line, then one slide per part with its
//! heading and a real table; a part with more than [`ROWS_PER_SLIDE`] rows goes on as many slides
//! as it needs, each repeating the column names. Slides added to an existing presentation use its
//! slide size and a layout of its own (its blank layout when it has one), so its own slides, masters
//! and layouts are left exactly as they were.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use super::ooxml::{app_props, content_types, core_props, esc, root_rels, Package, XML_HEAD};
use super::{Part, Sheet};

const P_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const SLIDE_TYPE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const SLIDE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
/// 16:9, 13.333 × 7.5 inches, in EMU.
const WIDE: (u64, u64) = (12_192_000, 6_858_000);
/// Data rows on one slide.
pub(super) const ROWS_PER_SLIDE: usize = 12;
/// The largest presentation read to add slides to: 1 GiB in all, uncompressed.
const MAX_PRESENTATION_BYTES: u64 = 1 << 30;

pub(super) fn write(sheet: &Sheet) -> Vec<u8> {
    let slides = slides(sheet, WIDE);
    let mut package = Package::new();
    let mut overrides = vec![
        (
            "/ppt/presentation.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
        ),
        (
            "/ppt/slideMasters/slideMaster1.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml",
        ),
        (
            "/ppt/slideLayouts/slideLayout1.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml",
        ),
        (
            "/ppt/theme/theme1.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.theme+xml",
        ),
        (
            "/ppt/presProps.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml",
        ),
        (
            "/ppt/tableStyles.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml",
        ),
    ];
    for i in 1..=slides.len() {
        overrides.push((format!("/ppt/slides/slide{i}.xml"), SLIDE_TYPE));
    }
    package.add("[Content_Types].xml", content_types(&overrides).as_bytes());
    package.add(
        "_rels/.rels",
        root_rels("officeDocument", "ppt/presentation.xml").as_bytes(),
    );
    package.add("docProps/core.xml", core_props(&sheet.title).as_bytes());
    package.add("docProps/app.xml", app_props().as_bytes());

    let mut ids = String::new();
    let mut rels = format!(
        r#"{XML_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps" Target="presProps.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles" Target="tableStyles.xml"/>"#
    );
    for i in 1..=slides.len() {
        let rid = i + 4;
        ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{rid}"/>"#, 255 + i));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{rid}" Type="{SLIDE_REL}" Target="slides/slide{i}.xml"/>"#
        ));
    }
    rels.push_str("</Relationships>");
    package.add(
        "ppt/presentation.xml",
        format!(
            r#"{XML_HEAD}<p:presentation xmlns:a="{A_NS}" xmlns:r="{R_NS}" xmlns:p="{P_NS}"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="{}" cy="{}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#,
            WIDE.0, WIDE.1
        )
        .as_bytes(),
    );
    package.add("ppt/_rels/presentation.xml.rels", rels.as_bytes());
    package.add(
        "ppt/presProps.xml",
        format!(
            r#"{XML_HEAD}<p:presentationPr xmlns:a="{A_NS}" xmlns:r="{R_NS}" xmlns:p="{P_NS}"/>"#
        )
        .as_bytes(),
    );
    package.add(
        "ppt/tableStyles.xml",
        format!(r#"{XML_HEAD}<a:tblStyleLst xmlns:a="{A_NS}" def="{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}"/>"#)
            .as_bytes(),
    );
    package.add("ppt/slideMasters/slideMaster1.xml", master().as_bytes());
    package.add(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        format!(
            r#"{XML_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"#
        )
        .as_bytes(),
    );
    package.add("ppt/slideLayouts/slideLayout1.xml", layout().as_bytes());
    package.add(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        format!(
            r#"{XML_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#
        )
        .as_bytes(),
    );
    package.add("ppt/theme/theme1.xml", theme().as_bytes());
    for (i, slide) in slides.iter().enumerate() {
        let n = i + 1;
        package.add(&format!("ppt/slides/slide{n}.xml"), slide.as_bytes());
        package.add(
            &format!("ppt/slides/_rels/slide{n}.xml.rels"),
            slide_rels("slideLayout1.xml").as_bytes(),
        );
    }
    package.finish()
}

/// Why slides could not be added to a presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AddSlidesError {
    /// Not a zip package, protected with a password, too large, or missing the parts a presentation
    /// has (`DSK_PRESENTATION_UNREADABLE`).
    #[error("DSK_PRESENTATION_UNREADABLE")]
    Unreadable,
}

/// The bytes of `presentation` with `sheet`'s slides added after its last slide. Every other part
/// is copied unchanged; only `ppt/presentation.xml`, its relationships and `[Content_Types].xml`
/// gain the new slides' entries.
pub fn add_slides(
    presentation: &[u8],
    name: &str,
    report: &ordin_types::domain::Report,
    words: &super::Words,
) -> Result<Vec<u8>, AddSlidesError> {
    let sheet = super::lay_out(name, report, words);
    add_sheet(presentation, &sheet)
}

pub(super) fn add_sheet(presentation: &[u8], sheet: &Sheet) -> Result<Vec<u8>, AddSlidesError> {
    let parts = read_parts(presentation)?;
    let text = |name: &str| {
        parts
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, bytes)| String::from_utf8(bytes.clone()).ok())
            .ok_or(AddSlidesError::Unreadable)
    };
    let main = text("ppt/presentation.xml")?;
    let main_rels = text("ppt/_rels/presentation.xml.rels")?;
    let types = text("[Content_Types].xml")?;
    if !main.contains("<p:presentation") || !main.contains("xmlns:r=") || !main.contains("<p:sldSz")
    {
        return Err(AddSlidesError::Unreadable);
    }
    let size = slide_size(&main).unwrap_or(WIDE);
    let layout = choose_layout(&parts).ok_or(AddSlidesError::Unreadable)?;

    let first_number = parts
        .iter()
        .filter_map(|(n, _)| {
            n.strip_prefix("ppt/slides/slide")?
                .strip_suffix(".xml")?
                .parse::<u64>()
                .ok()
        })
        .max()
        .unwrap_or(0)
        + 1;
    let first_id = attribute_values(&main, "<p:sldId ", "id")
        .into_iter()
        .max()
        .unwrap_or(255)
        .max(255)
        + 1;

    let new_slides = slides(sheet, size);
    let taken: std::collections::HashSet<String> = id_values(&main_rels).into_iter().collect();
    let mut rid_number = 0u64;
    let mut ids = String::new();
    let mut rels = String::new();
    let mut overrides = String::new();
    let mut added: Vec<(String, Vec<u8>)> = Vec::new();
    for (i, slide) in (0u64..).zip(&new_slides) {
        let n = first_number + i;
        let next_id = first_id + i;
        let rid = loop {
            rid_number += 1;
            let candidate = format!("rId{rid_number}");
            if !taken.contains(&candidate) {
                break candidate;
            }
        };
        ids.push_str(&format!(r#"<p:sldId id="{next_id}" r:id="{rid}"/>"#));
        rels.push_str(&format!(
            r#"<Relationship Id="{rid}" Type="{SLIDE_REL}" Target="slides/slide{n}.xml"/>"#
        ));
        overrides.push_str(&format!(
            r#"<Override PartName="/ppt/slides/slide{n}.xml" ContentType="{SLIDE_TYPE}"/>"#
        ));
        added.push((
            format!("ppt/slides/slide{n}.xml"),
            slide.clone().into_bytes(),
        ));
        added.push((
            format!("ppt/slides/_rels/slide{n}.xml.rels"),
            slide_rels(&layout).into_bytes(),
        ));
    }

    let main = if let Some(at) = main.find("</p:sldIdLst>") {
        format!("{}{ids}{}", &main[..at], &main[at..])
    } else if let Some(at) = main.find("<p:sldIdLst/>") {
        format!(
            "{}<p:sldIdLst>{ids}</p:sldIdLst>{}",
            &main[..at],
            &main[at + "<p:sldIdLst/>".len()..]
        )
    } else {
        let at = main.find("<p:sldSz").ok_or(AddSlidesError::Unreadable)?;
        format!(
            "{}<p:sldIdLst>{ids}</p:sldIdLst>{}",
            &main[..at],
            &main[at..]
        )
    };
    let main_rels = insert_before(&main_rels, "</Relationships>", &rels)?;
    let types = insert_before(&types, "</Types>", &overrides)?;

    let mut package = Package::new();
    for (name, bytes) in &parts {
        match name.as_str() {
            "ppt/presentation.xml" => package.add(name, main.as_bytes()),
            "ppt/_rels/presentation.xml.rels" => package.add(name, main_rels.as_bytes()),
            "[Content_Types].xml" => package.add(name, types.as_bytes()),
            _ => package.add(name, bytes),
        }
    }
    for (name, bytes) in &added {
        package.add(name, bytes);
    }
    Ok(package.finish())
}

fn insert_before(text: &str, end: &str, insert: &str) -> Result<String, AddSlidesError> {
    let at = text.rfind(end).ok_or(AddSlidesError::Unreadable)?;
    Ok(format!("{}{insert}{}", &text[..at], &text[at..]))
}

/// Every file of the package, in its order; directories are skipped.
fn read_parts(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, AddSlidesError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| AddSlidesError::Unreadable)?;
    let mut parts = Vec::with_capacity(archive.len());
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|_| AddSlidesError::Unreadable)?;
        if file.is_dir() {
            continue;
        }
        if file.encrypted() {
            return Err(AddSlidesError::Unreadable);
        }
        let name = file
            .name()
            .map_err(|_| AddSlidesError::Unreadable)?
            .into_owned();
        let mut content = Vec::new();
        (&mut file)
            .take(MAX_PRESENTATION_BYTES - total + 1)
            .read_to_end(&mut content)
            .map_err(|_| AddSlidesError::Unreadable)?;
        total += content.len() as u64;
        if total > MAX_PRESENTATION_BYTES {
            return Err(AddSlidesError::Unreadable);
        }
        parts.push((name, content));
    }
    Ok(parts)
}

/// Every `Id="…"` value of a relationships part.
fn id_values(rels: &str) -> Vec<String> {
    rels.split(" Id=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_owned)
        .collect()
}

/// The values of `attribute` on every element that starts with `element` (e.g. `<p:sldId `).
fn attribute_values(xml: &str, element: &str, attribute: &str) -> Vec<u64> {
    let mut values = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(element) {
        rest = &rest[at + element.len()..];
        let tag = &rest[..rest.find('>').unwrap_or(rest.len())];
        if let Some(value) = attribute_in(tag, attribute).and_then(|v| v.parse().ok()) {
            values.push(value);
        }
    }
    values
}

/// The value of ` attribute="…"` in one tag's text.
fn attribute_in<'a>(tag: &'a str, attribute: &str) -> Option<&'a str> {
    let key = format!(" {attribute}=\"");
    let start = tag.find(&key).map(|at| at + key.len()).or_else(|| {
        // The first attribute follows the element name directly after its space.
        tag.strip_prefix(&key[1..]).map(|_| key.len() - 1)
    })?;
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// The presentation's slide size from `<p:sldSz cx="…" cy="…"/>`.
fn slide_size(main: &str) -> Option<(u64, u64)> {
    let at = main.find("<p:sldSz")?;
    let tag = &main[at + "<p:sldSz".len()..];
    let tag = &tag[..tag.find('>')?];
    let cx = attribute_in(tag, "cx")?.parse().ok()?;
    let cy = attribute_in(tag, "cy")?.parse().ok()?;
    (cx > 0 && cy > 0).then_some((cx, cy))
}

/// The layout the new slides use: the presentation's blank layout, else its first by number.
pub(super) fn choose_layout(parts: &[(String, Vec<u8>)]) -> Option<String> {
    let layouts: BTreeMap<u64, (&str, &[u8])> = parts
        .iter()
        .filter_map(|(name, bytes)| {
            let file = name.strip_prefix("ppt/slideLayouts/")?;
            let n = file
                .strip_prefix("slideLayout")?
                .strip_suffix(".xml")?
                .parse()
                .ok()?;
            Some((n, (file, bytes.as_slice())))
        })
        .collect();
    layouts
        .values()
        .find(|(_, bytes)| {
            std::str::from_utf8(bytes).is_ok_and(|text| text.contains(r#"type="blank""#))
        })
        .or_else(|| layouts.values().next())
        .map(|(file, _)| (*file).to_owned())
}

fn slide_rels(layout_file: &str) -> String {
    format!(
        r#"{XML_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/{layout_file}"/></Relationships>"#
    )
}

/// The slides of `sheet` for a slide of `size` (EMU).
pub(super) fn slides(sheet: &Sheet, size: (u64, u64)) -> Vec<String> {
    let (cx, cy) = size;
    let margin = cx / 24;
    let width = cx - 2 * margin;
    let mut out = Vec::new();
    // The title slide.
    let mut shapes = text_box(
        2,
        "Title",
        margin,
        cy * 3 / 10,
        width,
        cy / 5,
        &sheet.title,
        4000,
        true,
    );
    shapes.push_str(&text_box(
        3,
        "Subtitle",
        margin,
        cy / 2,
        width,
        cy / 10,
        &sheet.subtitle,
        1800,
        false,
    ));
    out.push(slide(&shapes));
    for part in &sheet.parts {
        let chunks: Vec<&[Vec<super::Cell>]> = if part.rows.is_empty() {
            vec![&[]]
        } else {
            part.rows.chunks(ROWS_PER_SLIDE).collect()
        };
        for rows in chunks {
            let top = cy / 20;
            let heading_height = cy / 8;
            let mut shapes = text_box(
                2,
                "Heading",
                margin,
                top,
                width,
                heading_height,
                &part.heading,
                2800,
                true,
            );
            let table_top = top + heading_height + cy / 40;
            shapes.push_str(&table(
                part,
                rows,
                margin,
                table_top,
                width,
                cy - table_top - cy / 20,
            ));
            out.push(slide(&shapes));
        }
    }
    out
}

fn slide(shapes: &str) -> String {
    format!(
        r#"{XML_HEAD}<p:sld xmlns:a="{A_NS}" xmlns:r="{R_NS}" xmlns:p="{P_NS}"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
    )
}

#[allow(clippy::too_many_arguments)]
fn text_box(
    id: u32,
    name: &str,
    x: u64,
    y: u64,
    cx: u64,
    cy: u64,
    text: &str,
    size: u32,
    bold: bool,
) -> String {
    let b = if bold { r#" b="1""# } else { "" };
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name} {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square" rtlCol="0" anchor="b"><a:normAutofit/></a:bodyPr><a:lstStyle/><a:p><a:r><a:rPr lang="en-GB" sz="{size}"{b} dirty="0"/><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>"#,
        esc(text)
    )
}

fn table(part: &Part, rows: &[Vec<super::Cell>], x: u64, y: u64, cx: u64, max_cy: u64) -> String {
    let columns = part.columns.len().max(1);
    let col_width = cx / columns as u64;
    let row_height = (max_cy / (ROWS_PER_SLIDE as u64 + 1)).min(370_840);
    let size = match columns {
        0..=4 => 1400,
        5..=7 => 1200,
        _ => 1000,
    };
    let mut out = format!(
        r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="3" name="Table 3"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{}" cy="{}"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr firstRow="1" bandRow="1"/><a:tblGrid>"#,
        col_width * columns as u64,
        row_height * (rows.len() as u64 + 1)
    );
    for _ in 0..columns {
        out.push_str(&format!(r#"<a:gridCol w="{col_width}"/>"#));
    }
    out.push_str("</a:tblGrid>");
    out.push_str(&format!(r#"<a:tr h="{row_height}">"#));
    for c in 0..columns {
        let name = part.columns.get(c).map_or("", String::as_str);
        out.push_str(&table_cell(name, size, true));
    }
    out.push_str("</a:tr>");
    for row in rows {
        out.push_str(&format!(r#"<a:tr h="{row_height}">"#));
        for c in 0..columns {
            out.push_str(&table_cell(
                row.get(c).map_or("", |cell| cell.shown()),
                size,
                false,
            ));
        }
        out.push_str("</a:tr>");
    }
    out.push_str("</a:tbl></a:graphicData></a:graphic></p:graphicFrame>");
    out
}

fn table_cell(text: &str, size: u32, header: bool) -> String {
    let b = if header { r#" b="1""# } else { "" };
    let paragraph = if text.is_empty() {
        format!(r#"<a:p><a:endParaRPr lang="en-GB" sz="{size}" dirty="0"/></a:p>"#)
    } else {
        format!(
            r#"<a:p><a:r><a:rPr lang="en-GB" sz="{size}"{b} dirty="0"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:rPr><a:t>{}</a:t></a:r></a:p>"#,
            esc(text)
        )
    };
    let fill = if header {
        r#"<a:solidFill><a:srgbClr val="E7E6E6"/></a:solidFill>"#
    } else {
        "<a:noFill/>"
    };
    format!(
        r#"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{paragraph}</a:txBody><a:tcPr><a:lnB w="12700"><a:solidFill><a:srgbClr val="BFBFBF"/></a:solidFill></a:lnB>{fill}</a:tcPr></a:tc>"#
    )
}

fn master() -> String {
    format!(
        r#"{XML_HEAD}<p:sldMaster xmlns:a="{A_NS}" xmlns:r="{R_NS}" xmlns:p="{P_NS}"><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles></p:sldMaster>"#
    )
}

fn layout() -> String {
    format!(
        r#"{XML_HEAD}<p:sldLayout xmlns:a="{A_NS}" xmlns:r="{R_NS}" xmlns:p="{P_NS}" type="blank" preserve="1"><p:cSld name="Blank"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#
    )
}

fn theme() -> String {
    let colour =
        |name: &str, hex: &str| format!(r#"<a:{name}><a:srgbClr val="{hex}"/></a:{name}>"#);
    let colours: String = [
        ("dk1", "000000"),
        ("lt1", "FFFFFF"),
        ("dk2", "44546A"),
        ("lt2", "E7E6E6"),
        ("accent1", "4472C4"),
        ("accent2", "ED7D31"),
        ("accent3", "A5A5A5"),
        ("accent4", "FFC000"),
        ("accent5", "5B9BD5"),
        ("accent6", "70AD47"),
        ("hlink", "0563C1"),
        ("folHlink", "954F72"),
    ]
    .iter()
    .map(|(name, hex)| colour(name, hex))
    .collect();
    let fonts = r#"<a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/>"#;
    let fill = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#;
    let line = r#"<a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#;
    let effect = "<a:effectStyle><a:effectLst/></a:effectStyle>";
    format!(
        r#"{XML_HEAD}<a:theme xmlns:a="{A_NS}" name="ordin"><a:themeElements><a:clrScheme name="ordin">{colours}</a:clrScheme><a:fontScheme name="ordin"><a:majorFont>{fonts}</a:majorFont><a:minorFont>{fonts}</a:minorFont></a:fontScheme><a:fmtScheme name="ordin"><a:fillStyleLst>{fill}{fill}{fill}</a:fillStyleLst><a:lnStyleLst>{line}{line}{line}</a:lnStyleLst><a:effectStyleLst>{effect}{effect}{effect}</a:effectStyleLst><a:bgFillStyleLst>{fill}{fill}{fill}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    )
}
