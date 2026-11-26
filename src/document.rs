use quick_xml::{Reader, events::Event};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Cursor, Read, Seek, Write},
    path::Path,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const MAX_FILE: u64 = 32 * 1024 * 1024;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum List {
    #[default]
    None,
    Bullet,
    Number,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub points: Option<u16>,
    pub color: Option<u32>,
    pub font: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Paragraph {
    pub runs: Vec<Run>,
    pub align: Align,
    pub list: List,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Rtf,
    Txt,
    Docx,
}

impl Format {
    pub fn detect(path: &Path) -> Option<Self> {
        Some(
            match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
                "rtf" => Self::Rtf,
                "txt" => Self::Txt,
                "docx" => Self::Docx,
                _ => return None,
            },
        )
    }
}

pub fn decode_text(bytes: &[u8]) -> io::Result<String> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Ожидается TXT в UTF-8 или UTF-16",
        )
    };
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let pairs = bytes[2..].chunks_exact(2);
        if !pairs.remainder().is_empty() {
            return Err(invalid());
        }
        let words = pairs.map(|pair| {
            if bytes[0] == 0xff {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        });
        return String::from_utf16(&words.collect::<Vec<_>>()).map_err(|_| invalid());
    }
    String::from_utf8(
        bytes
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(bytes)
            .to_vec(),
    )
    .map_err(|_| invalid())
}

pub fn save(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

pub fn open_docx(path: &Path) -> Result<Vec<Paragraph>> {
    read_docx(Cursor::new(read_file(path)?))
}

pub fn read_file(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let oversized = || io::Error::new(io::ErrorKind::InvalidData, "Документ слишком большой");
    if file.metadata()?.len() > MAX_FILE {
        return Err(oversized());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(oversized());
    }
    Ok(bytes)
}

pub fn checked_rtf(bytes: &[u8]) -> io::Result<&[u8]> {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let bytes = bytes.trim_ascii();
    // Some Rich Edit versions add a C string terminator when streaming a document out.
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes).trim_ascii();
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Некорректный документ RTF");
    if !bytes.starts_with(b"{\\rtf1") {
        return Err(invalid());
    }
    let mut depth = 0;
    let mut cursor = 0;
    while let Some(byte) = bytes.get(cursor) {
        cursor += 1;
        match byte {
            b'{' => {
                depth += 1;
                if depth > 256 {
                    return Err(invalid());
                }
            }
            b'}' => {
                depth -= 1;
                if depth == 0 && cursor != bytes.len() {
                    return Err(invalid());
                }
            }
            b'\\' => {
                let start = cursor;
                while bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
                    cursor += 1;
                }
                if cursor == start {
                    // A control symbol consumes the following byte even when it is a brace.
                    cursor += 1;
                    continue;
                }
                let name = &bytes[start..cursor];
                if [
                    b"object".as_slice(),
                    b"objdata",
                    b"objlink",
                    b"objautlink",
                    b"objocx",
                ]
                .iter()
                .any(|word| name.eq_ignore_ascii_case(word))
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Файл RTF содержит встроенный объект OLE",
                    ));
                }
                if name.eq_ignore_ascii_case(b"bin") {
                    let length_start = cursor;
                    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                        cursor += 1;
                    }
                    let length = std::str::from_utf8(&bytes[length_start..cursor])
                        .ok()
                        .and_then(|digits| digits.parse::<usize>().ok())
                        .ok_or_else(invalid)?;
                    if bytes.get(cursor) == Some(&b' ') {
                        cursor += 1;
                    }
                    cursor = cursor
                        .checked_add(length)
                        .filter(|&end| end <= bytes.len())
                        .ok_or_else(invalid)?;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(invalid());
    }
    Ok(bytes)
}

fn part<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Option<String>> {
    let mut file = match zip.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if file.size() > MAX_FILE {
        return Err("Документ слишком большой".into());
    }
    let mut xml = String::new();
    file.by_ref().take(MAX_FILE + 1).read_to_string(&mut xml)?;
    if xml.len() as u64 > MAX_FILE {
        return Err("Документ слишком большой".into());
    }
    Ok(Some(xml))
}

fn read_docx<R: Read + Seek>(source: R) -> Result<Vec<Paragraph>> {
    let mut archive = ZipArchive::new(source)?;
    let xml = part(&mut archive, "word/document.xml")?.ok_or("В DOCX нет основного документа")?;
    let lists = match part(&mut archive, "word/numbering.xml")? {
        Some(xml) => numbering(&xml)?,
        None => HashMap::new(),
    };
    let mut reader = Reader::from_str(&xml);
    let mut paragraphs = Vec::new();
    let mut paragraph = None::<Paragraph>;
    let mut run = None::<Run>;
    let mut in_text = false;
    let mut in_body = false;
    let mut found_body = false;
    let mut in_properties = false;
    let mut skip_depth = 0usize;
    let mut heading = None;
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Eof => break,
            Event::Start(_) if skip_depth > 0 => {
                skip_depth += 1;
                continue;
            }
            Event::End(_) if skip_depth > 0 => {
                skip_depth -= 1;
                continue;
            }
            _ if skip_depth > 0 => continue,
            _ => {}
        }
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                let opened = matches!(event, Event::Start(_));
                let name = element.local_name();
                let value = |key: &str| -> Option<String> {
                    element
                        .attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == key)
                        .and_then(|a| {
                            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .ok()
                                .map(|s| s.into_owned())
                        })
                };
                match name.as_ref() {
                    "body" => {
                        in_body = opened;
                        found_body = true;
                    }
                    "drawing" | "pict" | "object" | "del" | "moveFrom" | "AlternateContent" => {
                        skip_depth = usize::from(opened);
                    }
                    "p" if in_body => {
                        let next = Paragraph::default();
                        if opened {
                            paragraph = Some(next);
                            heading = None;
                        } else {
                            paragraphs.push(next);
                        }
                    }
                    "r" if paragraph.is_some() && opened => run = Some(Run::default()),
                    "pPr" if opened => in_properties = true,
                    "t" => in_text = opened && run.is_some(),
                    "tab" => {
                        if let Some(run) = &mut run {
                            run.text.push('\t');
                        }
                    }
                    "br" | "cr" => {
                        if let Some(run) = &mut run {
                            run.text.push('\n');
                        }
                    }
                    "noBreakHyphen" => {
                        if let Some(run) = &mut run {
                            run.text.push('‑');
                        }
                    }
                    "jc" if in_properties => {
                        if let Some(p) = &mut paragraph {
                            p.align = match value("val").as_deref() {
                                Some("center") => Align::Center,
                                Some("right" | "end") => Align::Right,
                                Some("both" | "distribute") => Align::Justify,
                                _ => Align::Left,
                            };
                        }
                    }
                    "numId" if in_properties => {
                        if let Some(p) = &mut paragraph {
                            p.list = value("val")
                                .and_then(|id| lists.get(&id).copied())
                                .unwrap_or_default();
                        }
                    }
                    "pStyle" if in_properties => {
                        heading = match value("val")
                            .unwrap_or_default()
                            .to_ascii_lowercase()
                            .as_str()
                        {
                            "title" | "heading1" => Some(44),
                            "heading2" => Some(34),
                            "heading3" => Some(28),
                            _ => None,
                        };
                    }
                    _ => {
                        if let Some(run) = &mut run {
                            let enabled = || {
                                !matches!(
                                    value("val").as_deref(),
                                    Some("0" | "false" | "off" | "none")
                                )
                            };
                            match name.as_ref() {
                                "b" => run.style.bold = enabled(),
                                "i" => run.style.italic = enabled(),
                                "u" => run.style.underline = enabled(),
                                "strike" | "dstrike" => run.style.strike = enabled(),
                                "sz" => {
                                    run.style.points = value("val")
                                        .and_then(|s| s.parse::<u16>().ok())
                                        .map(|n| n.clamp(12, 192))
                                }
                                "color" => {
                                    run.style.color = value("val")
                                        .and_then(|s| u32::from_str_radix(&s, 16).ok())
                                        .filter(|n| *n <= 0xffffff)
                                }
                                "rFonts" => {
                                    run.style.font = value("hAnsi")
                                        .or_else(|| value("ascii"))
                                        .filter(|s| s.chars().count() < 32)
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            Event::End(element) => match element.local_name().as_ref() {
                "body" => in_body = false,
                "t" => in_text = false,
                "pPr" => in_properties = false,
                "r" => {
                    if let (Some(p), Some(mut r)) = (&mut paragraph, run.take()) {
                        if let Some(size) = heading {
                            if r.style.points.is_none() {
                                r.style.points = Some(size);
                                r.style.bold = true;
                            }
                        }
                        if !r.text.is_empty() {
                            p.runs.push(r);
                        }
                    }
                }
                "p" => {
                    if let Some(p) = paragraph.take() {
                        paragraphs.push(p);
                    }
                }
                _ => {}
            },
            Event::Text(value) if in_text => {
                if let Some(r) = &mut run {
                    r.text.push_str(&value.xml10_content());
                }
            }
            Event::GeneralRef(reference) if in_text => {
                if let Some(r) = &mut run {
                    if let Some(ch) = reference.resolve_char_ref()? {
                        if valid_xml(ch) {
                            r.text.push(ch);
                        }
                    } else if let Some(s) =
                        quick_xml::escape::resolve_xml_entity(reference.as_ref())
                    {
                        r.text.push_str(s);
                    }
                }
            }
            Event::DocType(_) => return Err("DTD в документе не поддерживается".into()),
            _ => {}
        }
    }
    if !xml.contains("wordprocessingml") || !found_body || in_body || paragraph.is_some() {
        return Err("Неверная структура DOCX".into());
    }
    Ok(paragraphs)
}

fn numbering(xml: &str) -> Result<HashMap<String, List>> {
    let mut reader = Reader::from_str(xml);
    let mut definitions = HashMap::new();
    let mut references = HashMap::new();
    let mut definition = None::<String>;
    let mut number = None::<String>;
    loop {
        let event = reader.read_event()?;
        match event {
            Event::Eof => break,
            Event::Start(ref element) | Event::Empty(ref element) => {
                let value = |key: &str| -> Option<String> {
                    element
                        .attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == key)
                        .and_then(|a| {
                            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .ok()
                                .map(|s| s.into_owned())
                        })
                };
                match element.local_name().as_ref() {
                    "abstractNum" => definition = value("abstractNumId"),
                    "num" => number = value("numId"),
                    "numFmt" => {
                        if let Some(id) = &definition {
                            let list = match value("val").as_deref() {
                                Some("bullet") => List::Bullet,
                                Some("none") | None => List::None,
                                _ => List::Number,
                            };
                            definitions.entry(id.clone()).or_insert(list);
                        }
                    }
                    "abstractNumId" => {
                        if let (Some(id), Some(value)) = (&number, value("val")) {
                            references.insert(id.clone(), value);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(element) => match element.local_name().as_ref() {
                "abstractNum" => definition = None,
                "num" => number = None,
                _ => {}
            },
            _ => {}
        }
    }
    Ok(references
        .into_iter()
        .filter_map(|(id, reference)| definitions.get(&reference).map(|list| (id, *list)))
        .collect())
}

const CT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#;
const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
const DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/></Relationships>"#;
const NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

pub fn write_docx(paragraphs: &[Paragraph]) -> Result<Vec<u8>> {
    use std::fmt::Write as _;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, content) in [
        ("[Content_Types].xml", CT),
        ("_rels/.rels", ROOT_RELS),
        ("word/_rels/document.xml.rels", DOC_RELS),
    ] {
        zip.start_file(name, options)?;
        zip.write_all(content.as_bytes())?;
    }
    let mut numbers =
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><w:numbering xmlns:w="{NS}">"#);
    for (id, kind, pattern) in [(1, "bullet", "•"), (2, "decimal", "%1.")] {
        write!(
            numbers,
            r#"<w:abstractNum w:abstractNumId="{id}"><w:multiLevelType w:val="singleLevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="{kind}"/><w:lvlText w:val="{pattern}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="480" w:hanging="240"/></w:pPr></w:lvl></w:abstractNum>"#
        )?;
    }
    numbers.push_str(r#"<w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="2"/></w:num></w:numbering>"#);
    zip.start_file("word/numbering.xml", options)?;
    zip.write_all(numbers.as_bytes())?;

    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{NS}"><w:body>"#
    );
    for paragraph in paragraphs {
        let alignment = match paragraph.align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "both",
        };
        xml.push_str("<w:p><w:pPr>");
        if paragraph.list != List::None {
            let id = if paragraph.list == List::Bullet { 1 } else { 2 };
            write!(
                xml,
                r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="{id}"/></w:numPr>"#
            )?;
        }
        write!(xml, r#"<w:jc w:val="{alignment}"/></w:pPr>"#)?;
        for run in &paragraph.runs {
            let s = &run.style;
            xml.push_str("<w:r><w:rPr>");
            if let Some(font) = &s.font {
                let font: String = font.chars().filter(|&ch| valid_xml(ch)).collect();
                let font = quick_xml::escape::escape(&font);
                write!(xml, r#"<w:rFonts w:ascii="{font}" w:hAnsi="{font}"/>"#)?;
            }
            if s.bold {
                xml.push_str("<w:b/>");
            }
            if s.italic {
                xml.push_str("<w:i/>");
            }
            if s.strike {
                xml.push_str("<w:strike/>");
            }
            if let Some(color) = s.color {
                write!(xml, r#"<w:color w:val="{color:06X}"/>"#)?;
            }
            if let Some(size) = s.points {
                write!(xml, r#"<w:sz w:val="{size}"/>"#)?;
            }
            if s.underline {
                xml.push_str(r#"<w:u w:val="single"/>"#);
            }
            xml.push_str("</w:rPr>");
            for part in run.text.split_inclusive(['\t', '\r', '\n']) {
                let end = part.chars().last();
                let body = part.trim_end_matches(['\t', '\r', '\n']);
                if !body.is_empty() {
                    let body: String = body.chars().filter(|&ch| valid_xml(ch)).collect();
                    write!(
                        xml,
                        r#"<w:t xml:space="preserve">{}</w:t>"#,
                        quick_xml::escape::escape(&body)
                    )?;
                }
                match end {
                    Some('\t') => xml.push_str("<w:tab/>"),
                    Some('\r' | '\n') => xml.push_str("<w:br/>"),
                    _ => {}
                }
            }
            xml.push_str("</w:r>");
        }
        xml.push_str("</w:p>");
    }
    xml.push_str(r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1360" w:right="1360" w:bottom="1360" w:left="1360" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#);
    zip.start_file("word/document.xml", options)?;
    zip.write_all(xml.as_bytes())?;
    Ok(zip.finish()?.into_inner())
}

fn valid_xml(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

pub fn to_rtf(paragraphs: &[Paragraph]) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut fonts = vec!["Segoe UI"];
    let mut colors = Vec::new();
    for run in paragraphs.iter().flat_map(|p| &p.runs) {
        if let Some(font) = run
            .style
            .font
            .as_deref()
            .filter(|font| !font.contains(';') && !fonts.contains(font))
        {
            fonts.push(font);
        }
        if let Some(color) = run.style.color.filter(|color| !colors.contains(color)) {
            colors.push(color);
        }
    }
    let mut rtf = String::from(r"{\rtf1\ansi\ansicpg1252\deff0\uc1{\fonttbl");
    for (index, font) in fonts.iter().enumerate() {
        let _ = write!(rtf, r"{{\f{index}\fnil ");
        rtf_text(&mut rtf, font);
        rtf.push_str(";}");
    }
    rtf.push_str(r"}{\colortbl;");
    for color in &colors {
        let _ = write!(
            rtf,
            r"\red{}\green{}\blue{};",
            color >> 16,
            color >> 8 & 255,
            color & 255
        );
    }
    rtf.push('}');
    for (index, paragraph) in paragraphs.iter().enumerate() {
        if index != 0 {
            rtf.push_str(r"\par");
        }
        rtf.push_str(r"\pard\sa130\sl280\slmult1");
        rtf.push_str(match paragraph.align {
            Align::Left => r"\ql",
            Align::Center => r"\qc",
            Align::Right => r"\qr",
            Align::Justify => r"\qj",
        });
        rtf.push(' ');
        for run in &paragraph.runs {
            let style = &run.style;
            let font = style
                .font
                .as_deref()
                .and_then(|name| fonts.iter().position(|font| *font == name))
                .unwrap_or(0);
            let color = style
                .color
                .and_then(|value| colors.iter().position(|entry| *entry == value))
                .map_or(0, |position| position + 1);
            let _ = write!(
                rtf,
                r"{{\f{font}\fs{}\cf{color}",
                style.points.unwrap_or(24)
            );
            if style.bold {
                rtf.push_str(r"\b");
            }
            if style.italic {
                rtf.push_str(r"\i");
            }
            if style.underline {
                rtf.push_str(r"\ul");
            }
            if style.strike {
                rtf.push_str(r"\strike");
            }
            rtf.push(' ');
            rtf_text(&mut rtf, &run.text);
            rtf.push('}');
        }
    }
    rtf.push('}');
    rtf.into_bytes()
}

fn rtf_text(rtf: &mut String, text: &str) {
    use std::fmt::Write as _;
    for unit in text.encode_utf16() {
        match unit {
            9 => rtf.push_str(r"\tab "),
            10 | 13 => rtf.push_str(r"\line "),
            92 | 123 | 125 => {
                rtf.push('\\');
                rtf.push(unit as u8 as char);
            }
            32..=126 => rtf.push(unit as u8 as char),
            0..=31 => {}
            _ => {
                let _ = write!(rtf, "\\u{}?", unit as i16);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docx_preserves_supported_content() {
        let original = vec![
            Paragraph {
                align: Align::Center,
                list: List::Number,
                runs: vec![Run {
                    text: " Привет & <мир> 😀\nстрока\tтабуляция ".into(),
                    style: Style {
                        bold: true,
                        italic: true,
                        underline: true,
                        strike: true,
                        points: Some(32),
                        color: Some(0x123456),
                        font: Some("Times New Roman".into()),
                    },
                }],
            },
            Paragraph::default(),
        ];
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Тест.DOCX");
        save(&path, b"previous document").unwrap();
        save(&path, &write_docx(&original).unwrap()).unwrap();
        assert_eq!(Format::detect(&path), Some(Format::Docx));
        assert_eq!(open_docx(&path).unwrap(), original);
    }

    #[test]
    fn unicode_text_decoding_and_rtf_escaping() {
        let input = "Текст {\\} 😀";
        let mut utf16 = vec![0xff, 0xfe];
        utf16.extend(input.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode_text(&utf16).unwrap(), input);
        assert!(decode_text(&utf16[..utf16.len() - 1]).is_err());
        let encoded = to_rtf(&[Paragraph {
            runs: vec![Run {
                text: input.into(),
                style: Style::default(),
            }],
            ..Paragraph::default()
        }]);
        let encoded = String::from_utf8(encoded).unwrap();
        assert!(encoded.contains(r"\{\\\}"));
        assert!(encoded.ends_with(r"\u-10179?\u-8704?}}"));
    }

    #[test]
    fn checks_rtf_before_passing_it_to_richedit() {
        assert!(checked_rtf(br"{\rtf1 \b Text \\object \{literal\}}").is_ok());
        assert_eq!(checked_rtf(b"{\\rtf1 Text}\0").unwrap(), br"{\rtf1 Text}");
        assert!(checked_rtf(br"{\rtf1 {\pict\bin4 {}{}}}").is_ok());
        assert!(checked_rtf(br"{\rtf1 {\*\ObjEcT\objemb test}}").is_err());
        assert!(checked_rtf(br"{\rtf1 {\*\objdata test}}").is_err());
        assert!(checked_rtf(br"{\rtf1 {\pict\bin42 data}}").is_err());
        assert!(checked_rtf(br"{\rtf1 text").is_err());
        assert!(checked_rtf(br"{\rtf1 text}{\object}").is_err());
    }
}
