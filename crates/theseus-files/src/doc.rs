//! Documents as text (theseus-c9l6): Word, Excel, PowerPoint, OpenDocument,
//! EPUB, RTF, and Jupyter notebooks, each read into sections a model reads
//! (a sheet, a slide with its title and notes, a notebook cell with its
//! outputs, a chapter), with a notebook's output images kept as images. An
//! archive's section is its list of members. Bounded everywhere: a member
//! read at most [`MAX_MEMBER_BYTES`], a sheet at most [`MAX_ROWS`] rows, the
//! whole text at most [`crate::pdf::MAX_TEXT_BYTES`].

use std::io::Read as _;

use serde::{Deserialize, Serialize};

use crate::kind::Kind;
use crate::xml::{self, Item};

/// The most one member of a zip is read to: an Office file's XML, a
/// notebook's JSON. A zip bomb's member stops here.
pub const MAX_MEMBER_BYTES: u64 = 64 * 1024 * 1024;

/// The most rows of one sheet that are read.
pub const MAX_ROWS: usize = 5_000;

/// The most members an archive's list names.
pub const MAX_LISTED: usize = 2_000;

/// One part of a document, as a model reads it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// What it is: `sheet Budget`, `slide 3: Tides`, `cell 4 (code)`.
    pub label: String,
    pub text: String,
    /// Images it holds: a notebook cell's outputs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Picture>,
}

/// An image a document holds, as its bytes (the core stores it in the blobs).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Picture {
    pub media_type: String,
    #[serde(with = "b64")]
    pub bytes: Vec<u8>,
}

pub(crate) mod b64 {
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(serde::de::Error::custom)
    }
}

/// A document read into its sections.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    pub sections: Vec<Section>,
    /// What was left out to stay within the bounds, in words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<String>,
}

/// The names of a zip's members, for [`crate::kind::sniff`]; empty for
/// anything that is not a zip.
pub fn zip_members(bytes: &[u8]) -> Vec<String> {
    let Ok(z) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Vec::new();
    };
    z.file_names().map(str::to_string).collect()
}

type Zip<'a> = zip::ZipArchive<std::io::Cursor<&'a [u8]>>;

fn open_zip(bytes: &[u8]) -> Result<Zip<'_>, String> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("it could not be read as a zip ({e})"))
}

/// A member's bytes, at most [`MAX_MEMBER_BYTES`] of them; `None` when the
/// zip has no such member.
fn member_bytes(z: &mut Zip<'_>, name: &str) -> Result<Option<Vec<u8>>, String> {
    let mut f = match z.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(format!("its member {name} could not be read ({e})")),
    };
    if f.size() > MAX_MEMBER_BYTES {
        return Err(format!(
            "its member {name} unpacks to {} bytes, over the {} MiB a member is read to",
            f.size(),
            MAX_MEMBER_BYTES / (1024 * 1024)
        ));
    }
    let mut out = Vec::new();
    (&mut f)
        .take(MAX_MEMBER_BYTES + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("its member {name} could not be read ({e})"))?;
    if out.len() as u64 > MAX_MEMBER_BYTES {
        return Err(format!("its member {name} unpacks past its stated size"));
    }
    Ok(Some(out))
}

fn member_text(z: &mut Zip<'_>, name: &str) -> Result<Option<String>, String> {
    Ok(member_bytes(z, name)?.map(|b| String::from_utf8_lossy(&b).into_owned()))
}

/// Read `bytes`, of kind `kind`, into sections.
pub fn read(bytes: &[u8], kind: Kind) -> Result<Doc, String> {
    let mut doc = match kind {
        Kind::Docx => docx(bytes)?,
        Kind::Xlsx => xlsx(bytes)?,
        Kind::Pptx => pptx(bytes)?,
        Kind::Odt => odf(bytes, Odf::Text)?,
        Kind::Ods => odf(bytes, Odf::Sheets)?,
        Kind::Odp => odf(bytes, Odf::Slides)?,
        Kind::Epub => epub(bytes)?,
        Kind::Rtf => one("text", rtf(bytes)),
        Kind::Notebook => notebook(bytes)?,
        Kind::Zip | Kind::Tar | Kind::TarGz => crate::archive::listing(bytes, kind)?,
        Kind::Pdf | Kind::Audio | Kind::Video | Kind::Other => {
            return Err(format!("a {} is not read as a document", kind.noun()))
        }
    };
    bound(&mut doc);
    Ok(doc)
}

fn one(label: &str, text: String) -> Doc {
    Doc {
        sections: vec![Section {
            label: label.into(),
            text,
            images: vec![],
        }],
        cut: None,
    }
}

/// Keep the whole text within [`crate::pdf::MAX_TEXT_BYTES`].
fn bound(doc: &mut Doc) {
    let max = crate::pdf::MAX_TEXT_BYTES;
    let mut held = 0usize;
    for (i, s) in doc.sections.iter_mut().enumerate() {
        if held + s.text.len() > max {
            let mut end = max - held;
            while !s.text.is_char_boundary(end) {
                end -= 1;
            }
            s.text.truncate(end);
            let total = doc.sections.len();
            doc.sections.truncate(i + 1);
            doc.cut = Some(format!(
                "its text stops in section {} of {total}, at {} MiB",
                i + 1,
                max / (1024 * 1024)
            ));
            return;
        }
        held += s.text.len();
    }
}

/// Lines of a paragraph-by-paragraph document: blank lines between
/// paragraphs collapse to one.
fn tidy(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        out.push_str(line);
        blank = 0;
    }
    out
}

// ------------------------------------------------------------------- Word

fn docx(bytes: &[u8]) -> Result<Doc, String> {
    let mut z = open_zip(bytes)?;
    let body = member_text(&mut z, "word/document.xml")?
        .ok_or("it has no word/document.xml, so it is not a Word document")?;
    let mut out = String::new();
    let mut para = String::new();
    let (mut heading, mut listed, mut in_table) = (0usize, false, 0usize);
    let mut in_text = false;
    for it in xml::items(&body) {
        match it {
            Item::Open { name, attrs, empty } => match Item::local(name) {
                "p" if !empty => {
                    para.clear();
                    (heading, listed) = (0, false);
                }
                "pStyle" => {
                    let v = xml::attr(&attrs, "val").unwrap_or("").to_ascii_lowercase();
                    heading = heading_level(&v);
                }
                "numPr" => listed = true,
                "t" if !empty => in_text = true,
                "tab" => para.push('\t'),
                "br" | "cr" => para.push('\n'),
                "tbl" if !empty => in_table += 1,
                _ => {}
            },
            Item::Close { name } => match Item::local(name) {
                "t" => in_text = false,
                "p" => {
                    let p = para.trim().to_string();
                    if in_table > 0 {
                        if !p.is_empty() {
                            out.push_str(&p);
                            out.push(' ');
                        }
                    } else if !p.is_empty() {
                        if heading > 0 {
                            out.push_str(&"#".repeat(heading.min(6)));
                            out.push(' ');
                        } else if listed {
                            out.push_str("- ");
                        }
                        out.push_str(&p);
                        out.push_str("\n\n");
                    }
                }
                "tc" if in_table > 0 => out.push_str("| "),
                "tr" if in_table > 0 => out.push('\n'),
                "tbl" => {
                    in_table = in_table.saturating_sub(1);
                    out.push('\n');
                }
                _ => {}
            },
            Item::Text(t) if in_text => para.push_str(&t),
            Item::Text(_) => {}
        }
    }
    Ok(one("text", tidy(&out)))
}

/// `heading2` is 2, `title` 1, anything else 0.
fn heading_level(style: &str) -> usize {
    if style == "title" {
        return 1;
    }
    style
        .strip_prefix("heading")
        .and_then(|n| n.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

// ------------------------------------------------------------------ Excel

/// A relationships file's targets, by id.
fn rels(xml_text: &str) -> Vec<(String, String)> {
    xml::items(xml_text)
        .into_iter()
        .filter_map(|it| match it {
            Item::Open { name, attrs, .. } if Item::local(name) == "Relationship" => Some((
                xml::attr(&attrs, "Id")?.to_string(),
                xml::attr(&attrs, "Target")?.to_string(),
            )),
            _ => None,
        })
        .collect()
}

/// A column's index from a cell reference: `C7` is 2.
fn column(r: &str) -> usize {
    r.chars()
        .take_while(char::is_ascii_alphabetic)
        .fold(0usize, |n, c| {
            n * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1)
        })
        .saturating_sub(1)
}

fn xlsx(bytes: &[u8]) -> Result<Doc, String> {
    let mut z = open_zip(bytes)?;
    let book = member_text(&mut z, "xl/workbook.xml")?
        .ok_or("it has no xl/workbook.xml, so it is not an Excel workbook")?;
    let links = rels(&member_text(&mut z, "xl/_rels/workbook.xml.rels")?.unwrap_or_default());
    let shared = shared_strings(&member_text(&mut z, "xl/sharedStrings.xml")?.unwrap_or_default());
    let mut sections = Vec::new();
    let mut cut = None;
    for it in xml::items(&book) {
        let Item::Open { name, attrs, .. } = it else {
            continue;
        };
        if Item::local(name) != "sheet" {
            continue;
        }
        let title = xml::attr(&attrs, "name").unwrap_or("sheet").to_string();
        let id = xml::attr(&attrs, "id").unwrap_or("");
        let Some((_, target)) = links.iter().find(|(i, _)| i == id) else {
            continue;
        };
        let path = match target.strip_prefix('/') {
            Some(abs) => abs.to_string(),
            None => format!("xl/{}", target.trim_start_matches("./")),
        };
        let Some(sheet) = member_text(&mut z, &path)? else {
            continue;
        };
        let (table, more) = sheet_rows(&sheet, &shared);
        if more {
            cut = Some(format!("a sheet's rows past {MAX_ROWS} are left out"));
        }
        sections.push(Section {
            label: format!("sheet {title}"),
            text: table,
            images: vec![],
        });
    }
    Ok(Doc { sections, cut })
}

fn shared_strings(x: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut cur, mut in_si, mut in_t) = (String::new(), false, false);
    for it in xml::items(x) {
        match it {
            Item::Open { name, empty, .. } => match Item::local(name) {
                "si" if !empty => (cur, in_si) = (String::new(), true),
                "t" if !empty && in_si => in_t = true,
                _ => {}
            },
            Item::Close { name } => match Item::local(name) {
                "si" => {
                    out.push(std::mem::take(&mut cur));
                    in_si = false;
                }
                "t" => in_t = false,
                _ => {}
            },
            Item::Text(t) if in_t => cur.push_str(&t),
            Item::Text(_) => {}
        }
    }
    out
}

/// A sheet's rows, each a line of its cells joined by ` | `, and whether
/// rows were left out.
fn sheet_rows(x: &str, shared: &[String]) -> (String, bool) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let (mut col, mut kind, mut val, mut in_v, mut in_t) =
        (0usize, String::new(), String::new(), false, false);
    for it in xml::items(x) {
        match it {
            Item::Open { name, attrs, empty } => match Item::local(name) {
                "row" if !empty => row.clear(),
                "c" => {
                    col = xml::attr(&attrs, "r").map_or(row.len(), column);
                    kind = xml::attr(&attrs, "t").unwrap_or("").to_string();
                    val.clear();
                }
                "v" if !empty => in_v = true,
                "t" if !empty => in_t = true,
                _ => {}
            },
            Item::Close { name } => match Item::local(name) {
                "v" => in_v = false,
                "t" => in_t = false,
                "c" => {
                    let shown = match kind.as_str() {
                        "s" => val
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .and_then(|i| shared.get(i).cloned())
                            .unwrap_or_default(),
                        "b" => if val.trim() == "1" { "TRUE" } else { "FALSE" }.to_string(),
                        _ => val.trim().to_string(),
                    };
                    if row.len() <= col {
                        row.resize(col + 1, String::new());
                    }
                    row[col] = shown;
                }
                "row" => {
                    rows.push(std::mem::take(&mut row));
                    if rows.len() > MAX_ROWS {
                        rows.truncate(MAX_ROWS);
                        return (table_text(&rows), true);
                    }
                }
                _ => {}
            },
            Item::Text(t) if in_v || in_t => val.push_str(&t),
            Item::Text(_) => {}
        }
    }
    (table_text(&rows), false)
}

/// Rows as lines, cells joined by ` | `, trailing empty cells and rows
/// dropped.
fn table_text(rows: &[Vec<String>]) -> String {
    let lines: Vec<String> = rows
        .iter()
        .map(|r| {
            let n = r
                .iter()
                .rposition(|c| !c.trim().is_empty())
                .map_or(0, |i| i + 1);
            r[..n].join(" | ")
        })
        .collect();
    let n = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(0, |i| i + 1);
    lines[..n].join("\n")
}

// ------------------------------------------------------------- PowerPoint

fn pptx(bytes: &[u8]) -> Result<Doc, String> {
    let mut z = open_zip(bytes)?;
    let mut slides: Vec<(usize, String)> = z
        .file_names()
        .filter_map(|n| {
            let num = n
                .strip_prefix("ppt/slides/slide")?
                .strip_suffix(".xml")?
                .parse::<usize>()
                .ok()?;
            Some((num, n.to_string()))
        })
        .collect();
    slides.sort();
    let mut sections = Vec::new();
    for (num, path) in slides {
        let Some(x) = member_text(&mut z, &path)? else {
            continue;
        };
        let (title, body) = slide_text(&x);
        let rels_path = format!("ppt/slides/_rels/slide{num}.xml.rels");
        let notes_target = rels(&member_text(&mut z, &rels_path)?.unwrap_or_default())
            .into_iter()
            .find(|(_, t)| t.contains("notesSlide"))
            .map(|(_, t)| format!("ppt/notesSlides/{}", t.rsplit('/').next().unwrap_or("")));
        let notes = match notes_target {
            Some(p) => member_text(&mut z, &p)?
                .map(|n| slide_text(&n).1)
                .unwrap_or_default(),
            None => String::new(),
        };
        let mut text = body;
        if !notes.trim().is_empty() {
            text.push_str(&format!("\n\nNotes: {}", notes.trim()));
        }
        let label = match title.trim() {
            "" => format!("slide {num}"),
            t => format!("slide {num}: {t}"),
        };
        sections.push(Section {
            label,
            text: text.trim().to_string(),
            images: vec![],
        });
    }
    Ok(Doc {
        sections,
        cut: None,
    })
}

/// A slide's title (its title placeholder's text) and the rest of its
/// text, a paragraph a line. On a notes slide, only the notes' body.
fn slide_text(x: &str) -> (String, String) {
    let (mut title, mut body) = (String::new(), String::new());
    let mut para = String::new();
    let (mut in_t, mut is_title, mut skip) = (false, false, false);
    for it in xml::items(x) {
        match it {
            Item::Open { name, attrs, empty } => match Item::local(name) {
                "sp" if !empty => (is_title, skip) = (false, false),
                "ph" => {
                    let t = xml::attr(&attrs, "type").unwrap_or("");
                    is_title = matches!(t, "title" | "ctrTitle");
                    skip = matches!(t, "sldNum" | "sldImg" | "dt" | "ftr" | "hdr");
                }
                "p" if !empty => para.clear(),
                "t" if !empty => in_t = true,
                "br" => para.push('\n'),
                _ => {}
            },
            Item::Close { name } => match Item::local(name) {
                "t" => in_t = false,
                "p" => {
                    let p = para.trim();
                    if !p.is_empty() && !skip {
                        let into = if is_title { &mut title } else { &mut body };
                        if !into.is_empty() {
                            into.push(if is_title { ' ' } else { '\n' });
                        }
                        into.push_str(p);
                    }
                }
                _ => {}
            },
            Item::Text(t) if in_t => para.push_str(&t),
            Item::Text(_) => {}
        }
    }
    (title, body)
}

// ----------------------------------------------------------- OpenDocument

#[derive(Clone, Copy, PartialEq)]
enum Odf {
    Text,
    Sheets,
    Slides,
}

#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
fn odf(bytes: &[u8], what: Odf) -> Result<Doc, String> {
    let mut z = open_zip(bytes)?;
    let x = member_text(&mut z, "content.xml")?
        .ok_or("it has no content.xml, so it is not an OpenDocument file")?;
    let mut sections: Vec<Section> = Vec::new();
    let mut cur = String::new();
    let mut label = String::new();
    let mut row: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cell = String::new();
    let (mut col_repeat, mut row_repeat, mut in_cell, mut in_notes) =
        (1usize, 1usize, false, false);
    let mut para = String::new();
    let mut heading = 0usize;
    let mut depth_p = 0usize;
    let mut list = 0usize;
    for it in xml::items(&x) {
        match it {
            Item::Open { name, attrs, empty } => match name {
                "table:table" if !empty && what == Odf::Sheets => {
                    label = format!(
                        "sheet {}",
                        xml::attr(&attrs, "table:name").unwrap_or("sheet")
                    );
                    rows.clear();
                }
                "draw:page" if !empty && what == Odf::Slides => {
                    label = format!("slide {}", sections.len() + 1);
                    if let Some(n) =
                        xml::attr(&attrs, "draw:name").filter(|n| !n.starts_with("page"))
                    {
                        label.push_str(&format!(": {n}"));
                    }
                    cur.clear();
                }
                "presentation:notes" if !empty => {
                    in_notes = true;
                    cur.push_str("\n\nNotes: ");
                }
                "table:table-row" => {
                    row.clear();
                    row_repeat = xml::attr(&attrs, "table:number-rows-repeated")
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(1);
                    if empty {
                        continue;
                    }
                }
                "table:table-cell" | "table:covered-table-cell" => {
                    col_repeat = xml::attr(&attrs, "table:number-columns-repeated")
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(1);
                    cell.clear();
                    in_cell = !empty;
                    if empty {
                        push_cells(&mut row, String::new(), col_repeat);
                    }
                }
                "text:h" if !empty => {
                    heading = xml::attr(&attrs, "text:outline-level")
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(1);
                    para.clear();
                    depth_p += 1;
                }
                "text:p" if !empty => {
                    para.clear();
                    depth_p += 1;
                }
                "text:list-item" if !empty => list += 1,
                "text:tab" => para.push('\t'),
                "text:line-break" => para.push('\n'),
                "text:s" => {
                    let n = xml::attr(&attrs, "text:c")
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(1usize);
                    para.push_str(&" ".repeat(n.min(64)));
                }
                _ => {}
            },
            Item::Close { name } => match name {
                "text:p" | "text:h" => {
                    depth_p = depth_p.saturating_sub(1);
                    let p = para.trim().to_string();
                    if in_cell {
                        if !cell.is_empty() && !p.is_empty() {
                            cell.push(' ');
                        }
                        cell.push_str(&p);
                    } else if !p.is_empty() {
                        if name == "text:h" {
                            cur.push_str(&"#".repeat(heading.clamp(1, 6)));
                            cur.push(' ');
                        } else if list > 0 && !in_notes {
                            cur.push_str("- ");
                        }
                        cur.push_str(&p);
                        cur.push_str(if in_notes { " " } else { "\n\n" });
                    }
                    para.clear();
                }
                "text:list-item" => list = list.saturating_sub(1),
                "table:table-cell" | "table:covered-table-cell" => {
                    in_cell = false;
                    push_cells(&mut row, std::mem::take(&mut cell), col_repeat);
                }
                "table:table-row" => {
                    if what == Odf::Sheets {
                        for _ in 0..row_repeat.min(64) {
                            rows.push(row.clone());
                        }
                        if rows.len() > MAX_ROWS {
                            rows.truncate(MAX_ROWS);
                        }
                    } else {
                        let line = row.iter().map(|c| c.trim()).collect::<Vec<_>>().join(" | ");
                        cur.push_str(line.trim_end_matches([' ', '|']));
                        cur.push('\n');
                    }
                }
                "table:table" if what == Odf::Sheets => sections.push(Section {
                    label: std::mem::take(&mut label),
                    text: table_text(&rows),
                    images: vec![],
                }),
                "presentation:notes" => in_notes = false,
                "draw:page" if what == Odf::Slides => sections.push(Section {
                    label: std::mem::take(&mut label),
                    text: cur.trim().to_string(),
                    images: vec![],
                }),
                _ => {}
            },
            Item::Text(t) if depth_p > 0 => para.push_str(&t),
            Item::Text(_) => {}
        }
    }
    if what == Odf::Text {
        sections.push(Section {
            label: "text".into(),
            text: tidy(&cur),
            images: vec![],
        });
    }
    Ok(Doc {
        sections,
        cut: None,
    })
}

/// A cell repeated `n` times, a trailing run of empty cells kept short:
/// OpenDocument repeats an empty cell to the sheet's last column.
fn push_cells(row: &mut Vec<String>, cell: String, n: usize) {
    let n = if cell.trim().is_empty() {
        n.min(64)
    } else {
        n.min(1024)
    };
    for _ in 0..n {
        row.push(cell.clone());
    }
}

// ------------------------------------------------------------------ EPUB

fn epub(bytes: &[u8]) -> Result<Doc, String> {
    let mut z = open_zip(bytes)?;
    let container = member_text(&mut z, "META-INF/container.xml")?
        .ok_or("it has no META-INF/container.xml, so it is not an EPUB")?;
    let opf_path = xml::items(&container)
        .into_iter()
        .find_map(|it| match it {
            Item::Open { name, attrs, .. } if Item::local(name) == "rootfile" => {
                xml::attr(&attrs, "full-path").map(str::to_string)
            }
            _ => None,
        })
        .ok_or("its container names no package file")?;
    let opf = member_text(&mut z, &opf_path)?.ok_or("its package file is missing")?;
    let base = opf_path.rsplit_once('/').map_or("", |(d, _)| d);
    let (mut manifest, mut spine) = (Vec::new(), Vec::new());
    for it in xml::items(&opf) {
        if let Item::Open { name, attrs, .. } = it {
            match Item::local(name) {
                "item" => manifest.push((
                    xml::attr(&attrs, "id").unwrap_or("").to_string(),
                    xml::attr(&attrs, "href").unwrap_or("").to_string(),
                )),
                "itemref" => spine.push(xml::attr(&attrs, "idref").unwrap_or("").to_string()),
                _ => {}
            }
        }
    }
    let mut sections = Vec::new();
    for idref in spine {
        let Some((_, href)) = manifest.iter().find(|(id, _)| *id == idref) else {
            continue;
        };
        let href = percent_decoded(href.split('#').next().unwrap_or(""));
        let path = if base.is_empty() {
            href
        } else {
            format!("{base}/{href}")
        };
        let Some(page) = member_text(&mut z, &path)? else {
            continue;
        };
        let (title, text) = xhtml_text(&page);
        if text.trim().is_empty() {
            continue;
        }
        let n = sections.len() + 1;
        sections.push(Section {
            label: match title {
                Some(t) => format!("chapter {n}: {t}"),
                None => format!("chapter {n}"),
            },
            text,
            images: vec![],
        });
    }
    Ok(Doc {
        sections,
        cut: None,
    })
}

fn percent_decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An XHTML page's first heading, and its text: a block a paragraph, a
/// heading marked as one, script and style left out.
fn xhtml_text(x: &str) -> (Option<String>, String) {
    let mut out = String::new();
    let mut title: Option<String> = None;
    let mut heading: Option<(usize, String)> = None;
    let mut skip = 0usize;
    let mut in_body = !x.contains("<body");
    for it in xml::items(x) {
        match it {
            Item::Open { name, empty, .. } => {
                let n = Item::local(name).to_ascii_lowercase();
                match n.as_str() {
                    "body" => in_body = true,
                    "script" | "style" | "head" if !empty => skip += 1,
                    "br" => out.push('\n'),
                    "li" if !empty => out.push_str("\n- "),
                    "p" | "div" | "tr" | "blockquote" | "section" => out.push_str("\n\n"),
                    h if h.len() == 2 && h.starts_with('h') && !empty => {
                        if let Some(l) =
                            h[1..].parse::<usize>().ok().filter(|l| (1..=6).contains(l))
                        {
                            heading = Some((l, String::new()));
                        }
                    }
                    "td" | "th" => out.push_str(" | "),
                    _ => {}
                }
            }
            Item::Close { name } => {
                let n = Item::local(name).to_ascii_lowercase();
                match n.as_str() {
                    "script" | "style" | "head" => skip = skip.saturating_sub(1),
                    h if h.len() == 2 && h.starts_with('h') => {
                        if let Some((l, t)) = heading.take() {
                            let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                            if title.is_none() && !t.is_empty() {
                                title = Some(t.clone());
                            }
                            out.push_str(&format!("\n\n{} {t}\n\n", "#".repeat(l)));
                        }
                    }
                    _ => {}
                }
            }
            Item::Text(t) if skip == 0 && in_body => match &mut heading {
                Some((_, h)) => h.push_str(&t),
                None => out.push_str(&t.replace('\n', " ")),
            },
            Item::Text(_) => {}
        }
    }
    (title, tidy(&out))
}

// ------------------------------------------------------------------- RTF

/// RTF's text: control words become what they mean (a paragraph, a tab, a
/// character), and the groups that are not text (fonts, colors, styles,
/// pictures, the info block) are skipped.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
pub fn rtf(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let b = s.as_bytes();
    let mut out = String::new();
    // Each group's state: whether it is skipped, and how many characters a
    // \uN takes the place of.
    let mut stack: Vec<(bool, usize)> = vec![(false, 1)];
    let mut skip_chars = 0usize;
    let mut i = 0;
    while i < b.len() {
        let skipping = stack.last().is_some_and(|g| g.0);
        match b[i] {
            b'{' => {
                let top = *stack.last().unwrap_or(&(false, 1));
                stack.push(top);
                i += 1;
            }
            b'}' => {
                if stack.len() > 1 {
                    stack.pop();
                }
                i += 1;
            }
            b'\\' => {
                i += 1;
                let Some(&c) = b.get(i) else { break };
                if c == b'\'' {
                    if let Some(h) = s
                        .get(i + 1..i + 3)
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                    {
                        if !skipping && skip_chars == 0 {
                            out.push(cp1252(h));
                        }
                        skip_chars = skip_chars.saturating_sub(1);
                    }
                    i += 3;
                    continue;
                }
                if !c.is_ascii_alphabetic() {
                    // A control symbol: \\, \{, \}, \~ (a space), \* (a
                    // destination to skip if unknown).
                    match c {
                        b'*' => {
                            if let Some(g) = stack.last_mut() {
                                g.0 = true;
                            }
                        }
                        b'~' if !skipping => out.push(' '),
                        b'\\' | b'{' | b'}' if !skipping => out.push(c as char),
                        _ => {}
                    }
                    i += 1;
                    continue;
                }
                let start = i;
                while i < b.len() && b[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let word = &s[start..i];
                let num_start = i;
                if i < b.len() && (b[i] == b'-' || b[i].is_ascii_digit()) {
                    i += 1;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let num: Option<i64> = s[num_start..i].parse().ok();
                if i < b.len() && b[i] == b' ' {
                    i += 1;
                }
                match word {
                    "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict" | "header"
                    | "footer" | "listtable" | "listoverridetable" | "themedata" | "datastore"
                    | "xmlnstbl" => {
                        if let Some(g) = stack.last_mut() {
                            g.0 = true;
                        }
                    }
                    "par" | "line" | "sect" | "page" if !skipping => out.push('\n'),
                    "tab" if !skipping => out.push('\t'),
                    "cell" if !skipping => out.push_str(" | "),
                    "row" if !skipping => out.push('\n'),
                    "uc" => {
                        if let Some(g) = stack.last_mut() {
                            g.1 = num.unwrap_or(1).max(0) as usize;
                        }
                    }
                    "u" => {
                        if let Some(n) = num {
                            let n = if n < 0 { n + 65_536 } else { n };
                            if !skipping {
                                if let Some(ch) = char::from_u32(n as u32) {
                                    out.push(ch);
                                }
                            }
                            skip_chars = stack.last().map_or(1, |g| g.1);
                        }
                    }
                    _ => {}
                }
            }
            b'\r' | b'\n' => i += 1,
            c => {
                if skip_chars > 0 {
                    skip_chars -= 1;
                } else if !skipping {
                    // Plain text, a character at a time (UTF-8 kept whole).
                    let ch_len = s[i..].chars().next().map_or(1, char::len_utf8);
                    out.push_str(&s[i..i + ch_len]);
                    i += ch_len;
                    continue;
                }
                let _ = c;
                i += 1;
            }
        }
    }
    tidy(&out)
}

/// A Windows-1252 byte as its character.
fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

// -------------------------------------------------------------- Notebooks

fn notebook(bytes: &[u8]) -> Result<Doc, String> {
    let nb: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| format!("it could not be read as a notebook's JSON ({e})"))?;
    let Some(cells) = nb["cells"].as_array() else {
        return Err(match nb["nbformat"].as_u64() {
            Some(n) if n < 4 => format!("it is an nbformat {n} notebook; only nbformat 4 is read"),
            _ => "it has no cells, so it is not a notebook".into(),
        });
    };
    let joined = |v: &serde_json::Value| match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str()).collect::<String>(),
        _ => String::new(),
    };
    let mut sections = Vec::new();
    for (i, cell) in cells.iter().enumerate() {
        let kind = cell["cell_type"].as_str().unwrap_or("code");
        let mut text = joined(&cell["source"]);
        let mut images = Vec::new();
        for out in cell["outputs"].as_array().into_iter().flatten() {
            let body = match out["output_type"].as_str().unwrap_or("") {
                "stream" => joined(&out["text"]),
                "error" => {
                    let tb = out["traceback"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|l| l.as_str())
                        .map(strip_ansi)
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "{}: {}\n{tb}",
                        out["ename"].as_str().unwrap_or("Error"),
                        out["evalue"].as_str().unwrap_or("")
                    )
                }
                _ => {
                    let data = &out["data"];
                    for (t, field) in [("image/png", "image/png"), ("image/jpeg", "image/jpeg")] {
                        if let Some(b64) = data[field].as_str() {
                            use base64::Engine as _;
                            let clean: String =
                                b64.chars().filter(|c| !c.is_whitespace()).collect();
                            if let Ok(bytes) =
                                base64::engine::general_purpose::STANDARD.decode(clean)
                            {
                                images.push(Picture {
                                    media_type: t.into(),
                                    bytes,
                                });
                            }
                        }
                    }
                    joined(&data["text/plain"])
                }
            };
            if !body.trim().is_empty() {
                text.push_str("\n[output]\n");
                text.push_str(body.trim_end());
            }
        }
        sections.push(Section {
            label: format!("cell {} ({kind})", i + 1),
            text,
            images,
        });
    }
    Ok(Doc {
        sections,
        cut: None,
    })
}

/// A traceback's line without its terminal colors.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Small documents for tests: a zip of `members`, each stored as given.
pub fn sample_zip(members: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write as _;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, body) in members {
        w.start_file(*name, opts).expect("a sample's member starts");
        w.write_all(body.as_bytes())
            .expect("a sample's member is written");
    }
    w.finish().expect("a sample zip ends").into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(d: &Doc) -> Vec<(String, String)> {
        d.sections
            .iter()
            .map(|s| (s.label.clone(), s.text.clone()))
            .collect()
    }

    #[test]
    fn a_word_document_reads_with_its_headings_lists_and_tables() {
        let docx = sample_zip(&[
            ("[Content_Types].xml", "<Types/>"),
            (
                "word/document.xml",
                r#"<w:document><w:body>
                <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Harbour rules</w:t></w:r></w:p>
                <w:p><w:r><w:t xml:space="preserve">Moorings are free </w:t></w:r><w:r><w:t>after six.</w:t></w:r></w:p>
                <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/></w:numPr></w:pPr><w:r><w:t>Pilot at the mark</w:t></w:r></w:p>
                <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Buoy</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Depth</w:t></w:r></w:p></w:tc></w:tr>
                <w:tr><w:tc><w:p><w:r><w:t>B-3</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>14.75</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
                </w:body></w:document>"#,
            ),
        ]);
        let d = read(&docx, Kind::Docx).unwrap();
        let t = &d.sections[0].text;
        assert!(
            t.starts_with("# Harbour rules\n\nMoorings are free after six.\n\n- Pilot at the mark"),
            "{t}"
        );
        assert!(
            t.contains("Buoy | Depth |") && t.contains("B-3 | 14.75 |"),
            "{t}"
        );
    }

    #[test]
    fn a_workbook_reads_as_its_sheets_tables_by_name() {
        let xlsx = sample_zip(&[
            (
                "xl/workbook.xml",
                r#"<workbook><sheets><sheet name="Soundings" sheetId="1" r:id="rId1"/><sheet name="Fees" sheetId="2" r:id="rId2"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Target="worksheets/sheet2.xml"/></Relationships>"#,
            ),
            (
                "xl/sharedStrings.xml",
                r#"<sst><si><t>Buoy</t></si><si><t>Depth</t></si><si><r><t>B-</t></r><r><t>3</t></r></si></sst>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="C2"><v>14.75</v></c></row></sheetData></worksheet>"#,
            ),
            (
                "xl/worksheets/sheet2.xml",
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>none</t></is></c><c r="B1" t="b"><v>1</v></c></row></sheetData></worksheet>"#,
            ),
        ]);
        let d = read(&xlsx, Kind::Xlsx).unwrap();
        assert_eq!(
            texts(&d),
            vec![
                (
                    "sheet Soundings".into(),
                    "Buoy | Depth\nB-3 |  | 14.75".into()
                ),
                ("sheet Fees".into(), "none | TRUE".into()),
            ]
        );
    }

    #[test]
    fn slides_read_with_their_titles_and_notes() {
        let pptx = sample_zip(&[
            (
                "ppt/slides/slide1.xml",
                r#"<p:sld><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Tides</a:t></a:r></a:p></p:txBody></p:sp>
                <p:sp><p:txBody><a:p><a:r><a:t>High water 06:12</a:t></a:r></a:p><a:p><a:r><a:t>Low water 12:30</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#,
            ),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                r#"<Relationships><Relationship Id="rId2" Target="../notesSlides/notesSlide1.xml"/></Relationships>"#,
            ),
            (
                "ppt/notesSlides/notesSlide1.xml",
                r#"<p:notes><p:sp><p:nvSpPr><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr></p:sp><p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Say the times twice.</a:t></a:r></a:p></p:txBody></p:sp></p:notes>"#,
            ),
            (
                "ppt/slides/slide2.xml",
                r#"<p:sld><p:sp><p:txBody><a:p><a:r><a:t>No title here</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#,
            ),
        ]);
        let d = read(&pptx, Kind::Pptx).unwrap();
        assert_eq!(
            texts(&d),
            vec![
                (
                    "slide 1: Tides".into(),
                    "High water 06:12\nLow water 12:30\n\nNotes: Say the times twice.".into()
                ),
                ("slide 2".into(), "No title here".into()),
            ]
        );
    }

    #[test]
    fn open_documents_read_as_text_sheets_and_slides() {
        let odt = sample_zip(&[
            ("mimetype", "application/vnd.oasis.opendocument.text"),
            (
                "content.xml",
                r#"<office:document-content><office:body><office:text><text:h text:outline-level="2">Fees</text:h><text:p>None<text:s text:c="2"/>at all.</text:p><text:list><text:list-item><text:p>Except dredging</text:p></text:list-item></text:list></office:text></office:body></office:document-content>"#,
            ),
        ]);
        assert_eq!(
            read(&odt, Kind::Odt).unwrap().sections[0].text,
            "## Fees\n\nNone  at all.\n\n- Except dredging"
        );
        let ods = sample_zip(&[(
            "content.xml",
            r#"<office:spreadsheet><table:table table:name="Tides"><table:table-row><table:table-cell><text:p>High</text:p></table:table-cell><table:table-cell table:number-columns-repeated="2"><text:p>6</text:p></table:table-cell><table:table-cell table:number-columns-repeated="1000"/></table:table-row><table:table-row table:number-rows-repeated="1048000"><table:table-cell table:number-columns-repeated="1024"/></table:table-row></table:table></office:spreadsheet>"#,
        )]);
        assert_eq!(
            texts(&read(&ods, Kind::Ods).unwrap()),
            vec![("sheet Tides".into(), "High | 6 | 6".into())]
        );
    }

    #[test]
    fn a_book_reads_chapter_by_chapter_in_its_spines_order() {
        let epub = sample_zip(&[
            ("mimetype", "application/epub+zip"),
            (
                "META-INF/container.xml",
                r#"<container><rootfiles><rootfile full-path="OEBPS/book.opf"/></rootfiles></container>"#,
            ),
            (
                "OEBPS/book.opf",
                r#"<package><manifest><item id="c1" href="one.xhtml"/><item id="c2" href="two%20b.xhtml"/></manifest><spine><itemref idref="c2"/><itemref idref="c1"/></spine></package>"#,
            ),
            (
                "OEBPS/one.xhtml",
                r#"<html><head><title>x</title><style>p{}</style></head><body><h1>The Mole</h1><p>It was the first day&#8217;s work.</p></body></html>"#,
            ),
            (
                "OEBPS/two b.xhtml",
                r#"<html><body><p>Prologue &amp; all.</p></body></html>"#,
            ),
        ]);
        assert_eq!(
            texts(&read(&epub, Kind::Epub).unwrap()),
            vec![
                ("chapter 1".into(), "Prologue & all.".into()),
                (
                    "chapter 2: The Mole".into(),
                    "# The Mole\n\nIt was the first day’s work.".into()
                ),
            ]
        );
    }

    #[test]
    fn rtf_reads_as_its_text() {
        let r = br"{\rtf1\ansi{\fonttbl{\f0 Times;}}{\colortbl;\red0;}\f0\pard Caf\'e9 opens at six.\par Pilots \u8212? board.\par{\*\generator Riched20;}}";
        assert_eq!(rtf(r), "Café opens at six.\nPilots — board.");
    }

    #[test]
    fn a_notebook_reads_cell_by_cell_with_outputs_and_images() {
        use base64::Engine as _;
        let png = base64::engine::general_purpose::STANDARD
            .encode(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x01\0\0\0\x01");
        let nb = serde_json::json!({
            "nbformat": 4, "metadata": {},
            "cells": [
                {"cell_type": "markdown", "source": ["# Soundings\n", "From the survey."]},
                {"cell_type": "code", "source": "print(14.75)", "outputs": [
                    {"output_type": "stream", "text": ["14.75\n"]},
                    {"output_type": "display_data", "data": {"text/plain": ["<Figure>"], "image/png": png}}
                ]},
                {"cell_type": "code", "source": "1/0", "outputs": [
                    {"output_type": "error", "ename": "ZeroDivisionError", "evalue": "division by zero",
                     "traceback": ["\u{1b}[0;31mZeroDivisionError\u{1b}[0m: division by zero"]}
                ]}
            ]
        });
        let d = read(nb.to_string().as_bytes(), Kind::Notebook).unwrap();
        assert_eq!(d.sections[0].label, "cell 1 (markdown)");
        assert_eq!(d.sections[0].text, "# Soundings\nFrom the survey.");
        assert_eq!(
            d.sections[1].text,
            "print(14.75)\n[output]\n14.75\n[output]\n<Figure>"
        );
        assert_eq!(d.sections[1].images.len(), 1);
        assert_eq!(d.sections[1].images[0].media_type, "image/png");
        assert!(
            d.sections[2]
                .text
                .ends_with("ZeroDivisionError: division by zero"),
            "{}",
            d.sections[2].text
        );
        assert!(
            read(b"{\"nbformat\": 3, \"worksheets\": []}", Kind::Notebook)
                .unwrap_err()
                .contains("nbformat 3")
        );
    }

    #[test]
    fn a_member_that_unpacks_past_the_cap_is_refused() {
        // A stored member is as long as its bytes; one declared longer than
        // the cap is refused before it is read.
        let bytes = sample_zip(&[("a.xml", "<a/>")]);
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        assert!(member_bytes(&mut z, "a.xml").unwrap().is_some());
        assert!(member_bytes(&mut z, "missing.xml").unwrap().is_none());
        assert!(read(b"not a zip", Kind::Docx)
            .unwrap_err()
            .starts_with("it could not be read as a zip"));
    }
}
