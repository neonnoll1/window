use crate::document::{self, Align, List, Paragraph, Run, Style};
use std::{io, mem, ptr, slice};
use windows_sys::Win32::{
    Foundation::{GlobalFree, HWND, POINT, RECT},
    Graphics::Gdi::*,
    Storage::Xps::*,
    UI::{
        Controls::{Dialogs::*, *},
        WindowsAndMessaging::*,
    },
};
use windows_sys::w;

pub const BOLD: u32 = 1;
pub const ITALIC: u32 = 2;
pub const UNDERLINE: u32 = 4;
pub const STRIKE: u32 = 8;
const SIZE: u32 = 0x8000_0000;
const COLOR: u32 = 0x4000_0000;
const FACE: u32 = 0x2000_0000;
const CHARSET: u32 = 0x0800_0000;
const ALL_STYLE: u32 = BOLD | ITALIC | UNDERLINE | STRIKE | SIZE | COLOR | FACE;
const P_NUMBER: u32 = 0x20;
const P_ALIGN: u32 = 8;
const P_AFTER: u32 = 0x80;
const P_LINE: u32 = 0x100;
const SCF_SELECTION: usize = 1;
const SF_RTF: usize = 2;
const SF_RTFNOOBJS: usize = 3;
const FIND_DOWN: usize = 1;
const LIMIT: usize = 2_000_000;
pub const EDITOR_ID: usize = 700;
pub const SELECTION_EVENT: u32 = 0x0702;

const GET_RANGE: u32 = WM_USER + 52;
const SET_RANGE: u32 = WM_USER + 55;
const FORMAT_RANGE: u32 = WM_USER + 57;
const GET_CHAR: u32 = WM_USER + 58;
const GET_PARA: u32 = WM_USER + 61;
const SET_CHAR: u32 = WM_USER + 68;
const SET_EVENTS: u32 = WM_USER + 69;
const SET_PARA: u32 = WM_USER + 71;
const STREAM_IN: u32 = WM_USER + 73;
const STREAM_OUT: u32 = WM_USER + 74;
const CAN_REDO: u32 = WM_USER + 85;
const REDO: u32 = WM_USER + 84;
const GET_TEXT: u32 = WM_USER + 94;
const GET_LENGTH: u32 = WM_USER + 95;
const FIND_TEXT: u32 = WM_USER + 124;
const GET_SCROLL: u32 = WM_USER + 221;
const SET_SCROLL: u32 = WM_USER + 222;
const SET_ZOOM: u32 = WM_USER + 225;

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Range {
    start: i32,
    end: i32,
}

#[repr(C)]
struct CharFormat {
    size: u32,
    mask: u32,
    effects: u32,
    height: i32,
    offset: i32,
    color: u32,
    charset: u8,
    family: u8,
    face: [u16; 32],
    weight: u16,
    spacing: i16,
    background: u32,
    locale: u32,
    reserved: u32,
    style: i16,
    kerning: u16,
    underline_type: u8,
    animation: u8,
    revision_author: u8,
    underline_color: u8,
}

impl Default for CharFormat {
    fn default() -> Self {
        let mut value: Self = unsafe { mem::zeroed() };
        value.size = mem::size_of::<Self>() as u32;
        value
    }
}

#[repr(C)]
struct ParaFormat {
    size: u32,
    mask: u32,
    numbering: u16,
    effects: u16,
    start_indent: i32,
    right_indent: i32,
    offset: i32,
    alignment: u16,
    tab_count: i16,
    tabs: [i32; 32],
    space_before: i32,
    space_after: i32,
    line_spacing: i32,
    style: i16,
    line_spacing_rule: u8,
    outline_level: u8,
    shading_weight: u16,
    shading_style: u16,
    numbering_start: u16,
    numbering_style: u16,
    numbering_tab: u16,
    border_space: u16,
    border_width: u16,
    borders: u16,
}

impl Default for ParaFormat {
    fn default() -> Self {
        let mut value: Self = unsafe { mem::zeroed() };
        value.size = mem::size_of::<Self>() as u32;
        value
    }
}

#[repr(C, packed(4))]
struct Stream {
    cookie: usize,
    error: u32,
    callback: unsafe extern "system" fn(usize, *mut u8, i32, *mut i32) -> u32,
}

struct Input<'a> {
    bytes: &'a [u8],
}

unsafe extern "system" fn read(cookie: usize, output: *mut u8, limit: i32, count: *mut i32) -> u32 {
    unsafe {
        let input = &mut *(cookie as *mut Input<'_>);
        let length = input.bytes.len().min(limit.max(0) as usize);
        ptr::copy_nonoverlapping(input.bytes.as_ptr(), output, length);
        input.bytes = &input.bytes[length..];
        *count = length as i32;
    }
    0
}

unsafe extern "system" fn write(
    cookie: usize,
    input: *mut u8,
    length: i32,
    count: *mut i32,
) -> u32 {
    unsafe {
        let output = &mut *(cookie as *mut Vec<u8>);
        if length < 0 || output.len() + length as usize > document::MAX_FILE as usize {
            *count = 0;
            return 1;
        }
        output.extend_from_slice(slice::from_raw_parts(input, length as usize));
        *count = length;
    }
    0
}

#[repr(C, packed(4))]
struct GetText {
    bytes: u32,
    flags: u32,
    codepage: u32,
    default_char: *const u8,
    used_default: *mut i32,
}

#[repr(C)]
struct GetLength {
    flags: u32,
    codepage: u32,
}

#[repr(C, packed(4))]
struct Find {
    range: Range,
    query: *const u16,
    result: Range,
}

#[repr(C)]
struct PrintRange {
    device: HDC,
    target: HDC,
    area: RECT,
    page: RECT,
    range: Range,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

#[derive(Default)]
pub struct Formatting {
    pub effects: u32,
    pub align: Align,
    pub list: List,
    pub points: Option<u16>,
    pub face: Option<String>,
}

#[derive(Clone, Copy)]
pub struct Editor(pub HWND);

impl Editor {
    pub fn create(parent: HWND) -> Self {
        unsafe {
            let handle = CreateWindowExW(
                0,
                w!("RICHEDIT50W"),
                w!("Документ"),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | ES_MULTILINE as u32
                    | ES_AUTOVSCROLL as u32
                    | ES_NOHIDESEL as u32
                    | ES_WANTRETURN as u32,
                0,
                0,
                0,
                0,
                parent,
                EDITOR_ID as _,
                ptr::null_mut(),
                ptr::null(),
            );
            let edit = Self(handle);
            if !handle.is_null() {
                edit.send(WM_USER + 67, 0, 0x00ff_ffff);
                edit.send(WM_USER + 53, 0, LIMIT as isize);
                edit.send(WM_USER + 82, 150, 0);
                edit.reset_defaults();
                edit.set_events(true);
            }
            edit
        }
    }

    fn send(self, message: u32, value: usize, data: isize) -> isize {
        unsafe { SendMessageW(self.0, message, value, data) }
    }

    fn reset_defaults(self) {
        let mut character = CharFormat {
            mask: ALL_STYLE | CHARSET,
            height: 240,
            color: 0x0039_3027,
            charset: 1,
            ..CharFormat::default()
        };
        let face = wide("Segoe UI");
        character.face[..face.len()].copy_from_slice(&face);
        self.send(SET_CHAR, 0, &character as *const _ as isize);
        self.send(SET_CHAR, SCF_SELECTION, &character as *const _ as isize);
        self.set_paragraph(&ParaFormat {
            mask: P_ALIGN | P_AFTER | P_LINE,
            alignment: 1,
            space_after: 130,
            line_spacing: 24,
            line_spacing_rule: 5,
            ..ParaFormat::default()
        });
    }

    pub fn set_events(self, enabled: bool) {
        self.send(SET_EVENTS, 0, if enabled { 0x0008_0001 } else { 0 });
    }

    pub fn clean(self) {
        self.send(EM_SETMODIFY, 0, 0);
    }

    pub fn modified(self) -> bool {
        self.send(EM_GETMODIFY, 0, 0) != 0
    }

    fn changed(self) {
        self.send(EM_SETMODIFY, 1, 0);
    }

    fn reset_history(self) {
        self.send(EM_EMPTYUNDOBUFFER, 0, 0);
        self.clean();
        self.set_range(Range::default());
    }

    pub fn text(self, text: &str) {
        self.set_events(false);
        let text = wide(text);
        self.send(WM_SETTEXT, 0, text.as_ptr() as isize);
        self.reset_defaults();
        self.reset_history();
        self.set_events(true);
    }

    pub fn load_rtf(self, bytes: &[u8]) -> io::Result<()> {
        // Check before touching the current document. Rich Edit accepts an RTF header.
        if !bytes.starts_with(b"{\\rtf") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Ожидается документ RTF",
            ));
        }
        let mut input = Input { bytes };
        let mut stream = Stream {
            cookie: &mut input as *mut _ as usize,
            error: 0,
            callback: read,
        };
        self.set_events(false);
        self.send(STREAM_IN, SF_RTF, &mut stream as *mut _ as isize);
        self.reset_history();
        self.set_events(true);
        if stream.error != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Не удалось прочитать RTF",
            ));
        }
        Ok(())
    }

    pub fn save_rtf(self) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::<u8>::new();
        let mut stream = Stream {
            cookie: &mut bytes as *mut _ as usize,
            error: 0,
            callback: write,
        };
        self.send(STREAM_OUT, SF_RTFNOOBJS, &mut stream as *mut _ as isize);
        if stream.error != 0 || bytes.is_empty() {
            return Err(io::Error::other("Не удалось сохранить содержимое RTF"));
        }
        Ok(bytes)
    }

    fn length(self) -> i32 {
        let info = GetLength {
            flags: 2 | 8,
            codepage: 1200,
        };
        self.send(GET_LENGTH, &info as *const _ as usize, 0).max(0) as i32
    }

    fn get_text(self, use_crlf: bool, raw: bool) -> Vec<u16> {
        let multiplier = if use_crlf { 2 } else { 1 };
        let mut output = vec![0u16; self.length() as usize * multiplier + 2];
        let parameters = GetText {
            bytes: (output.len() * 2) as u32,
            flags: u32::from(use_crlf) | if raw { 4 } else { 0 },
            codepage: 1200,
            default_char: ptr::null(),
            used_default: ptr::null_mut(),
        };
        let used = self.send(
            GET_TEXT,
            &parameters as *const _ as usize,
            output.as_mut_ptr() as isize,
        );
        output.truncate((used.max(0) as usize).min(output.len()));
        output
    }

    pub fn plain_text(self, use_crlf: bool) -> String {
        String::from_utf16_lossy(&self.get_text(use_crlf, false))
    }

    fn get_range(self) -> Range {
        let mut range = Range::default();
        self.send(GET_RANGE, 0, &mut range as *mut _ as isize);
        range
    }

    fn set_range(self, range: Range) {
        self.send(SET_RANGE, 0, &range as *const _ as isize);
    }

    fn get_char(self) -> CharFormat {
        let mut format = CharFormat::default();
        format.mask = self.send(GET_CHAR, SCF_SELECTION, &mut format as *mut _ as isize) as u32;
        format
    }

    fn get_paragraph(self) -> ParaFormat {
        let mut format = ParaFormat::default();
        self.send(GET_PARA, 0, &mut format as *mut _ as isize);
        format
    }

    fn set_paragraph(self, format: &ParaFormat) {
        self.send(SET_PARA, 0, format as *const _ as isize);
    }

    pub fn formatting(self) -> Formatting {
        let char = self.get_char();
        let paragraph = self.get_paragraph();
        Formatting {
            effects: char.effects & char.mask,
            align: alignment(paragraph.alignment),
            list: list_kind(paragraph.numbering),
            points: (char.mask & SIZE != 0).then_some((char.height / 10) as u16),
            face: (char.mask & FACE != 0).then(|| font_name(&char.face)),
        }
    }

    pub fn toggle(self, flag: u32) {
        let format = self.get_char();
        let active = format.mask & format.effects & flag != 0;
        self.apply_char(CharFormat {
            mask: flag,
            effects: if active { 0 } else { flag },
            ..Default::default()
        });
    }

    pub fn set_font(self, name: &str) {
        let mut format = CharFormat {
            mask: FACE | CHARSET,
            charset: 1,
            ..Default::default()
        };
        let face = wide(name);
        format.face[..face.len()].copy_from_slice(&face);
        self.apply_char(format);
    }

    pub fn set_size(self, points: u16) {
        self.apply_char(CharFormat {
            mask: SIZE,
            height: i32::from(points) * 20,
            ..Default::default()
        });
    }

    pub fn set_color(self, color: u32) {
        self.apply_char(CharFormat {
            mask: COLOR,
            color: rgb(color),
            ..Default::default()
        });
    }

    pub fn heading(self, points: u16, bold: bool) {
        let selected = self.get_range();
        if selected.start == selected.end {
            let text = self.get_text(false, true);
            let index = selected.start.clamp(0, text.len() as i32) as usize;
            let separator = |unit: &u16| matches!(*unit, 13 | 10);
            let start = text[..index]
                .iter()
                .rposition(separator)
                .map_or(0, |value| value + 1);
            let end = text[index..]
                .iter()
                .position(separator)
                .map_or(text.len(), |value| index + value);
            self.set_range(Range {
                start: start as i32,
                end: end as i32,
            });
        }
        self.apply_char(CharFormat {
            mask: SIZE | BOLD,
            effects: if bold { BOLD } else { 0 },
            height: i32::from(points) * 20,
            ..Default::default()
        });
        if selected.start == selected.end {
            let formatted = self.get_range();
            if formatted.start != formatted.end {
                self.set_range(selected);
            }
        }
    }

    fn apply_char(self, format: CharFormat) {
        let range = self.get_range();
        self.send(WM_USER + 88, 0, 0);
        self.send(SET_CHAR, SCF_SELECTION, &format as *const _ as isize);
        if range.start != range.end {
            self.changed();
        }
    }

    pub fn align(self, align: Align) {
        self.set_paragraph(&ParaFormat {
            mask: P_ALIGN,
            alignment: match align {
                Align::Left => 1,
                Align::Right => 2,
                Align::Center => 3,
                Align::Justify => 4,
            },
            ..Default::default()
        });
        self.changed();
    }

    pub fn list(self, list: List) {
        let current = self.get_paragraph();
        self.set_list(if list_kind(current.numbering) == list {
            List::None
        } else {
            list
        });
        self.changed();
    }

    pub fn restore_docx_lists(self, paragraphs: &[Paragraph]) {
        if paragraphs
            .iter()
            .all(|paragraph| paragraph.list == List::None)
        {
            return;
        }
        let text = self.get_text(false, true);
        self.set_events(false);
        let mut start = 0;
        for paragraph in paragraphs {
            if paragraph.list != List::None {
                self.set_range(Range {
                    start: start as i32,
                    end: start as i32,
                });
                self.set_list(paragraph.list);
            }
            let Some(end) = text[start..]
                .iter()
                .position(|unit| matches!(unit, 13 | 10))
            else {
                break;
            };
            let end = start + end;
            start = end
                + if text[end..].starts_with(&[13, 10]) {
                    2
                } else {
                    1
                };
        }
        self.reset_history();
        self.set_events(true);
    }

    fn set_list(self, list: List) {
        let numbering = match list {
            List::None => 0,
            List::Bullet => 1,
            List::Number => 2,
        };
        self.set_paragraph(&ParaFormat {
            mask: P_NUMBER | 1 | 4 | 0xe000,
            numbering,
            numbering_start: 1,
            numbering_style: 0x0200,
            numbering_tab: 350,
            start_indent: if numbering == 0 { 0 } else { 420 },
            offset: if numbering == 0 { 0 } else { 260 },
            ..Default::default()
        });
    }

    pub fn zoom(self, percent: i32) {
        self.send(SET_ZOOM, percent as usize, 100);
    }

    pub fn undo(self) {
        self.send(EM_UNDO, 0, 0);
    }

    pub fn redo(self) {
        self.send(REDO, 0, 0);
    }

    pub fn can_undo(self) -> bool {
        self.send(EM_CANUNDO, 0, 0) != 0
    }

    pub fn can_redo(self) -> bool {
        self.send(CAN_REDO, 0, 0) != 0
    }

    pub fn select_all(self) {
        self.set_range(Range { start: 0, end: -1 });
    }

    pub fn insert(self, text: &str) {
        let text = wide(text);
        self.send(EM_REPLACESEL, 1, text.as_ptr() as isize);
    }

    pub fn find(self, needle: &str) -> bool {
        let text = wide(needle);
        let range = self.get_range();
        let mut search = Find {
            range: Range {
                start: range.end,
                end: -1,
            },
            query: text.as_ptr(),
            result: Range::default(),
        };
        let mut found = self.send(FIND_TEXT, FIND_DOWN, &mut search as *mut _ as isize);
        if found < 0 && range.end > 0 {
            search.range = Range {
                start: 0,
                end: range.end,
            };
            found = self.send(FIND_TEXT, FIND_DOWN, &mut search as *mut _ as isize);
        }
        if found < 0 {
            return false;
        }
        self.set_range(search.result);
        self.send(EM_SCROLLCARET, 0, 0);
        true
    }

    pub fn paragraphs(self) -> Vec<Paragraph> {
        let text = self.get_text(false, true);
        let original = self.get_range();
        let mut scroll = POINT::default();
        self.send(GET_SCROLL, 0, &mut scroll as *mut _ as isize);
        self.set_events(false);
        self.send(WM_SETREDRAW, 0, 0);
        let mut paragraphs = Vec::new();
        let mut start = 0;
        loop {
            let end = text[start..]
                .iter()
                .position(|unit| matches!(unit, 13 | 10))
                .map_or(text.len(), |offset| start + offset);
            self.set_range(Range {
                start: start as i32,
                end: start as i32,
            });
            let format = self.get_paragraph();
            let mut paragraph = Paragraph {
                runs: Vec::new(),
                align: alignment(format.alignment),
                list: list_kind(format.numbering),
            };
            let mut cursor = start;
            while cursor < end {
                let mut lower = cursor + 1;
                let mut upper = end;
                while lower < upper {
                    let middle = lower + (upper - lower).div_ceil(2);
                    self.set_range(Range {
                        start: cursor as i32,
                        end: middle as i32,
                    });
                    if self.get_char().mask & ALL_STYLE == ALL_STYLE {
                        lower = middle;
                    } else {
                        upper = middle - 1;
                    }
                }
                if lower < end && (0xd800..=0xdbff).contains(&text[lower - 1]) {
                    lower += 1;
                }
                self.set_range(Range {
                    start: cursor as i32,
                    end: lower as i32,
                });
                let char = self.get_char();
                let effects = char.effects;
                paragraph.runs.push(Run {
                    text: String::from_utf16_lossy(&text[cursor..lower]).replace('\u{b}', "\n"),
                    style: Style {
                        bold: effects & BOLD != 0,
                        italic: effects & ITALIC != 0,
                        underline: effects & UNDERLINE != 0,
                        strike: effects & STRIKE != 0,
                        points: Some((char.height / 10).clamp(12, 192) as u16),
                        color: (effects & COLOR == 0).then_some(rgb(char.color)),
                        font: Some(font_name(&char.face)).filter(|face| !face.is_empty()),
                    },
                });
                cursor = lower;
            }
            paragraphs.push(paragraph);
            if end == text.len() {
                break;
            }
            start = end
                + if text[end..].starts_with(&[13, 10]) {
                    2
                } else {
                    1
                };
        }
        self.set_range(original);
        self.send(SET_SCROLL, 0, &scroll as *const _ as isize);
        self.send(WM_SETREDRAW, 1, 0);
        self.set_events(true);
        unsafe { InvalidateRect(self.0, ptr::null(), 1) };
        paragraphs
    }

    pub fn print(self, owner: HWND, title: &str) -> io::Result<bool> {
        unsafe {
            let mut dialog = PRINTDLGW {
                lStructSize: mem::size_of::<PRINTDLGW>() as u32,
                hwndOwner: owner,
                Flags: PD_RETURNDC | PD_NOSELECTION | PD_NOPAGENUMS | PD_USEDEVMODECOPIESANDCOLLATE,
                nCopies: 1,
                ..Default::default()
            };
            let accepted = PrintDlgW(&mut dialog);
            let error = if accepted == 0 {
                CommDlgExtendedError()
            } else {
                0
            };
            for handle in [dialog.hDevMode, dialog.hDevNames] {
                if !handle.is_null() {
                    GlobalFree(handle);
                }
            }
            let device = dialog.hDC;
            if accepted == 0 || device.is_null() {
                return if error == 0 {
                    Ok(false)
                } else {
                    Err(io::Error::other(format!(
                        "Ошибка диалога печати ({error:#x})"
                    )))
                };
            }
            let result = self.print_to(device, title);
            DeleteDC(device);
            result.map(|_| true)
        }
    }

    fn print_to(self, device: HDC, title: &str) -> io::Result<()> {
        unsafe {
            let horizontal = GetDeviceCaps(device, LOGPIXELSX as i32).max(1);
            let vertical = GetDeviceCaps(device, LOGPIXELSY as i32).max(1);
            let width = GetDeviceCaps(device, HORZRES as i32) * 1440 / horizontal;
            let height = GetDeviceCaps(device, VERTRES as i32) * 1440 / vertical;
            let title = wide(title);
            let info = DOCINFOW {
                cbSize: mem::size_of::<DOCINFOW>() as i32,
                lpszDocName: title.as_ptr(),
                ..Default::default()
            };
            let margin = 850.min(width / 5).min(height / 5);
            let last = self.length();
            let mut range = PrintRange {
                device,
                target: device,
                area: RECT {
                    left: margin,
                    top: margin,
                    right: width - margin,
                    bottom: height - margin,
                },
                page: RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
                range: Range {
                    start: 0,
                    end: last,
                },
            };
            if StartDocW(device, &info) <= 0 {
                return Err(io::Error::other("Принтер не принял документ"));
            }
            let mut problem = false;
            loop {
                if StartPage(device) <= 0 {
                    problem = true;
                    break;
                }
                let next = self.send(FORMAT_RANGE, 1, &mut range as *mut _ as isize) as i32;
                if EndPage(device) <= 0 || (next <= range.range.start && range.range.start < last) {
                    problem = true;
                    break;
                }
                if next >= last {
                    break;
                }
                range.range.start = next;
            }
            self.send(FORMAT_RANGE, 0, 0);
            if problem {
                AbortDoc(device);
                return Err(io::Error::other("Печать прервана"));
            }
            if EndDoc(device) <= 0 {
                return Err(io::Error::other("Не удалось закончить печать"));
            }
            Ok(())
        }
    }
}

fn alignment(value: u16) -> Align {
    match value {
        2 => Align::Right,
        3 => Align::Center,
        4 => Align::Justify,
        _ => Align::Left,
    }
}

fn list_kind(value: u16) -> List {
    match value {
        0 => List::None,
        1 => List::Bullet,
        _ => List::Number,
    }
}

fn font_name(face: &[u16]) -> String {
    String::from_utf16_lossy(
        &face[..face
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(face.len())],
    )
}

pub fn rgb(color: u32) -> u32 {
    (color & 255) << 16 | color & 0xff00 | (color >> 16) & 255
}
