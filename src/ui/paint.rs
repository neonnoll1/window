use super::*;
use crate::editor::rgb;

const WHITE: u32 = 0xffffff;
const CHROME: u32 = 0xfcfcfe;
const CANVAS: u32 = 0xf1f2f6;
const LINE: u32 = 0xe3e5ef;
const INK: u32 = 0x292b3d;
const MUTED: u32 = 0x686d81;
const ACCENT: u32 = 0x47477e;
const ACTIVE: u32 = 0xeeedf9;
const CLEAR: u32 = 0xf6f6fa;
const HOVER: u32 = 0xececf5;

#[derive(Clone, Copy)]
pub enum Glyph {
    None,
    Chevron,
    Plus,
    Minus,
    Folder,
    Save,
    Undo,
    Redo,
    Search,
    Close,
    Doc,
    Bold,
    Italic,
    Underline,
    Strike,
    Color,
    Left,
    Center,
    Right,
    Justify,
    Bullets,
    Numbers,
    Symbol,
    Calendar,
    Ruler,
    Book,
}

pub struct Fonts {
    pub body: HFONT,
    medium: HFONT,
    small: HFONT,
    brand: HFONT,
    bold: HFONT,
    italic: HFONT,
}

impl Fonts {
    pub fn new(dpi: u32) -> Self {
        let make = |size: i32, weight, italic| unsafe {
            CreateFontW(
                -(size * dpi as i32 + 48) / 96,
                0,
                0,
                0,
                weight,
                italic,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                w!("Segoe UI"),
            )
        };
        Self {
            body: make(14, 400, 0),
            medium: make(14, 600, 0),
            small: make(12, 400, 0),
            brand: make(23, 700, 0),
            bold: make(16, 700, 0),
            italic: make(17, 500, 1),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for font in [
            self.body,
            self.medium,
            self.small,
            self.brand,
            self.bold,
            self.italic,
        ] {
            unsafe {
                DeleteObject(font);
            }
        }
    }
}

struct Draw {
    dc: HDC,
    dpi: u32,
}

impl Draw {
    fn px(&self, value: i32) -> i32 {
        (value * self.dpi as i32 + 48) / 96
    }
    fn px_signed(&self, value: i32) -> i32 {
        value.signum() * self.px(value.abs())
    }

    fn fill(&self, rect: RECT, color: u32) {
        unsafe {
            let brush = CreateSolidBrush(rgb(color));
            FillRect(self.dc, &rect, brush);
            DeleteObject(brush);
        }
    }

    fn border(&self, rect: RECT, color: u32) {
        unsafe {
            let brush = CreateSolidBrush(rgb(color));
            FrameRect(self.dc, &rect, brush);
            DeleteObject(brush);
        }
    }

    fn rounded(&self, rect: RECT, radius: i32, fill: u32, edge: Option<u32>) {
        unsafe {
            let brush = CreateSolidBrush(rgb(fill));
            let pen = CreatePen(PS_SOLID, 1, rgb(edge.unwrap_or(fill)));
            let old_brush = SelectObject(self.dc, brush);
            let old_pen = SelectObject(self.dc, pen);
            RoundRect(
                self.dc,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                self.px(radius * 2),
                self.px(radius * 2),
            );
            SelectObject(self.dc, old_brush);
            SelectObject(self.dc, old_pen);
            DeleteObject(brush);
            DeleteObject(pen);
        }
    }

    fn text(&self, text: &str, mut rect: RECT, font: HFONT, color: u32, alignment: u32) {
        let text = wide(text);
        unsafe {
            let old_font = SelectObject(self.dc, font);
            SetBkMode(self.dc, TRANSPARENT as i32);
            SetTextColor(self.dc, rgb(color));
            DrawTextW(
                self.dc,
                text.as_ptr(),
                -1,
                &mut rect,
                DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS | alignment,
            );
            SelectObject(self.dc, old_font);
        }
    }

    fn text_width(&self, text: &str, font: HFONT) -> i32 {
        let text = wide(text);
        let mut size = SIZE::default();
        unsafe {
            let old_font = SelectObject(self.dc, font);
            GetTextExtentPoint32W(self.dc, text.as_ptr(), text.len() as i32 - 1, &mut size);
            SelectObject(self.dc, old_font);
        }
        size.cx
    }

    fn line(&self, points: &[(i32, i32)], color: u32, width: i32) {
        unsafe {
            let points = points
                .iter()
                .map(|&(x, y)| POINT { x, y })
                .collect::<Vec<_>>();
            let pen = CreatePen(PS_SOLID, width, rgb(color));
            let old = SelectObject(self.dc, pen);
            Polyline(self.dc, points.as_ptr(), points.len() as i32);
            SelectObject(self.dc, old);
            DeleteObject(pen);
        }
    }

    fn dot(&self, x: i32, y: i32, radius: i32, color: u32) {
        unsafe {
            let brush = CreateSolidBrush(rgb(color));
            let old = SelectObject(self.dc, brush);
            let previous = SelectObject(self.dc, GetStockObject(NULL_PEN));
            Ellipse(
                self.dc,
                x - radius,
                y - radius,
                x + radius + 1,
                y + radius + 1,
            );
            SelectObject(self.dc, previous);
            SelectObject(self.dc, old);
            DeleteObject(brush);
        }
    }

    fn glyph(&self, glyph: Glyph, x: i32, y: i32, fonts: &Fonts, color: u32) {
        let line = |path: &[(i32, i32)]| {
            let points = path
                .iter()
                .map(|&(dx, dy)| (x + self.px_signed(dx), y + self.px_signed(dy)))
                .collect::<Vec<_>>();
            self.line(&points, color, self.px(1));
        };
        let dot = |dx, dy| {
            self.dot(
                x + self.px_signed(dx),
                y + self.px_signed(dy),
                self.px(1),
                color,
            )
        };
        let letter = |text, font| {
            self.text(
                text,
                RECT {
                    left: x - self.px(11),
                    right: x + self.px(11),
                    top: y - self.px(11),
                    bottom: y + self.px(10),
                },
                font,
                color,
                DT_CENTER,
            )
        };
        match glyph {
            Glyph::None => {}
            Glyph::Chevron => line(&[(-4, -2), (0, 2), (5, -3)]),
            Glyph::Plus => {
                line(&[(-6, 0), (7, 0)]);
                line(&[(0, -6), (0, 7)]);
            }
            Glyph::Minus => line(&[(-6, 0), (7, 0)]),
            Glyph::Close => {
                line(&[(-5, -5), (6, 6)]);
                line(&[(-5, 5), (6, -6)]);
            }
            Glyph::Folder => line(&[
                (-8, 5),
                (-8, -6),
                (-2, -6),
                (0, -4),
                (7, -4),
                (7, 5),
                (-8, 5),
            ]),
            Glyph::Save => {
                line(&[(-7, -7), (4, -7), (7, -4), (7, 7), (-7, 7), (-7, -7)]);
                line(&[(-3, -7), (-3, -2), (3, -2), (3, -7)]);
                line(&[(-4, 7), (-4, 1), (4, 1), (4, 7)]);
            }
            Glyph::Undo | Glyph::Redo => {
                let flip = if matches!(glyph, Glyph::Undo) { 1 } else { -1 };
                let reflect = |coords: &[(i32, i32)]| {
                    line(
                        &coords
                            .iter()
                            .map(|&(x, y)| (x * flip, y))
                            .collect::<Vec<_>>(),
                    )
                };
                reflect(&[(-2, -6), (-7, -1), (-2, 4)]);
                reflect(&[(-7, -1), (3, -1), (5, 0), (7, 3), (7, 6)]);
            }
            Glyph::Search => {
                line(&[
                    (-7, -3),
                    (-6, -6),
                    (-3, -7),
                    (1, -7),
                    (4, -4),
                    (4, 0),
                    (1, 3),
                    (-3, 3),
                    (-6, 1),
                    (-7, -3),
                ]);
                line(&[(3, 2), (8, 7)]);
            }
            Glyph::Doc => {
                line(&[(-6, 8), (-6, -8), (2, -8), (7, -3), (7, 8), (-6, 8)]);
                line(&[(2, -8), (2, -3), (7, -3)]);
                line(&[(-3, 1), (4, 1)]);
                line(&[(-3, 4), (3, 4)]);
            }
            Glyph::Bold => letter("B", fonts.bold),
            Glyph::Italic => letter("I", fonts.italic),
            Glyph::Underline => {
                letter("U", fonts.medium);
                line(&[(-5, 7), (6, 7)]);
            }
            Glyph::Strike => {
                letter("S", fonts.medium);
                line(&[(-7, 0), (8, 0)]);
            }
            Glyph::Color => {
                letter("A", fonts.medium);
                self.line(
                    &[
                        (x - self.px(6), y + self.px(8)),
                        (x + self.px(7), y + self.px(8)),
                    ],
                    0x7975b4,
                    self.px(2),
                );
            }
            Glyph::Left | Glyph::Center | Glyph::Right | Glyph::Justify => {
                for (index, dy) in [-6, -2, 2, 6].into_iter().enumerate() {
                    let length = if index % 2 == 0 || matches!(glyph, Glyph::Justify) {
                        14
                    } else {
                        9
                    };
                    let left = match glyph {
                        Glyph::Right => 7 - length,
                        Glyph::Center => -length / 2,
                        _ => -7,
                    };
                    line(&[(left, dy), (left + length + 1, dy)]);
                }
            }
            Glyph::Bullets | Glyph::Numbers => {
                for dy in [-5, 0, 5] {
                    line(&[(-2, dy), (8, dy)]);
                    if matches!(glyph, Glyph::Bullets) {
                        dot(-6, dy);
                    } else {
                        line(&[(-8, dy - 1), (-7, dy - 1), (-7, dy + 2)]);
                    }
                }
            }
            Glyph::Symbol => letter("Ω", fonts.bold),
            Glyph::Calendar => {
                line(&[(-7, -5), (7, -5), (7, 7), (-7, 7), (-7, -5)]);
                line(&[(-7, -1), (8, -1)]);
                for dx in [-4, 4] {
                    line(&[(dx, -7), (dx, -3)]);
                }
                dot(-2, 3);
                dot(2, 3);
            }
            Glyph::Ruler => {
                line(&[(-8, -5), (8, -5), (8, 5), (-8, 5), (-8, -5)]);
                for dx in [-4, 0, 4] {
                    line(&[(dx, -5), (dx, if dx == 0 { 1 } else { -1 })]);
                }
            }
            Glyph::Book => {
                line(&[
                    (-8, -6),
                    (-3, -6),
                    (0, -4),
                    (3, -6),
                    (8, -6),
                    (8, 5),
                    (3, 5),
                    (0, 7),
                    (-3, 5),
                    (-8, 5),
                    (-8, -6),
                ]);
                line(&[(0, -4), (0, 7)]);
            }
        }
    }
}

fn code_mix(start: u32, end: u32, percent: u8) -> u32 {
    let step = u32::from(percent);
    let component = |shift| {
        (((start >> shift) & 255u32) * (255 - step) + ((end >> shift) & 255u32) * step) / 255u32
    };
    component(16) << 16 | component(8) << 8 | component(0)
}

impl App {
    pub fn draw_control(&self, item: &DRAWITEMSTRUCT) -> bool {
        let Some(control) = item
            .CtlID
            .checked_sub(BUTTON_ID as u32)
            .and_then(|index| self.buttons.get(index as usize))
        else {
            return false;
        };
        let dc = Draw {
            dc: item.hDC,
            dpi: self.dpi,
        };
        let rect = item.rcItem;
        let center_x = (rect.left + rect.right) / 2;
        let center_y = (rect.top + rect.bottom) / 2;
        let action = control.spec.action;
        let picked = match action {
            EditTab => self.tab == Tab::Edit,
            InsertTab => self.tab == Tab::Insert,
            ViewTab => self.tab == Tab::View,
            Find => self.show_search,
            Bold => self.format.effects & editor::BOLD != 0,
            Italic => self.format.effects & editor::ITALIC != 0,
            Underline => self.format.effects & editor::UNDERLINE != 0,
            Strike => self.format.effects & editor::STRIKE != 0,
            Center => self.format.align == Align::Center,
            Right => self.format.align == Align::Right,
            Justify => self.format.align == Align::Justify,
            Bullets => self.format.list == List::Bullet,
            Numbers => self.format.list == List::Number,
            Ruler => self.ruler,
            Reading => self.reading,
            _ => false,
        };
        let disabled = match action {
            Undo => !self.undo_available,
            Redo => !self.redo_available,
            ZoomOut => self.zoom == 50,
            ZoomIn => self.zoom == 180,
            Face | Size | Left | Center | Right | Justify | Bold | Italic | Underline | Strike
            | Color | Heading | Bullets | Numbers | Date | Symbol | Rule => self.reading,
            _ => false,
        };
        let parent_bg = match action {
            FindNext | CloseFind => CANVAS,
            ZoomOut | ZoomIn | ZoomReset | New | Undo | Redo | Open | Save | File | EditTab
            | InsertTab | ViewTab | Find => CHROME,
            _ => WHITE,
        };
        dc.fill(rect, parent_bg);
        let pressed = item.itemState & ODS_SELECTED != 0;
        let has_focus = item.itemState & ODS_FOCUS != 0;
        let kind = control.spec.kind;
        let fade = if disabled {
            0
        } else if pressed {
            255
        } else {
            control.hover
        };
        let tab_active = picked && kind == K::Tab;
        let (initial, target, edge, foreground) = match kind {
            K::Primary => (0x434477, 0x34356b, None, WHITE),
            K::Chip if picked => (ACTIVE, 0xe6e5f7, Some(0xdcdaf0), ACCENT),
            K::Chip => (CLEAR, HOVER, Some(LINE), INK),
            K::Normal if action == Open => (WHITE, CLEAR, Some(LINE), INK),
            K::Normal | K::Footer if picked => (ACTIVE, 0xe5e4f6, None, ACCENT),
            K::Tab if tab_active => (CHROME, CLEAR, None, ACCENT),
            K::Tab => (CHROME, CLEAR, None, MUTED),
            _ => (parent_bg, HOVER, None, INK),
        };
        let fill = code_mix(initial, target, fade);
        if fill != parent_bg || edge.is_some() || has_focus {
            let button_rect = if kind == K::Tab {
                RECT {
                    bottom: rect.bottom - self.px(4),
                    ..rect
                }
            } else {
                rect
            };
            dc.rounded(
                button_rect,
                8,
                fill,
                if has_focus { Some(0x8a89bf) } else { edge },
            );
        }
        if tab_active {
            dc.rounded(
                RECT {
                    left: center_x - self.px(22),
                    right: center_x + self.px(22),
                    top: rect.bottom - self.px(3),
                    bottom: rect.bottom,
                },
                1,
                ACCENT,
                None,
            );
        }
        let foreground = if disabled { 0xa3a7b8 } else { foreground };
        let label = match action {
            Face => self
                .format
                .face
                .as_deref()
                .filter(|face| !face.is_empty())
                .unwrap_or("Шрифт")
                .to_string(),
            Size => self.format.points.map_or_else(
                || "—".into(),
                |value| {
                    if value % 2 == 0 {
                        format!("{}", value / 2)
                    } else {
                        format!("{},5", value / 2)
                    }
                },
            ),
            ZoomReset => format!("{} %", self.zoom),
            Heading if self.format.points == Some(56) => "Заголовок".into(),
            Heading if self.format.points == Some(34) => "Подзаголовок".into(),
            _ => control.spec.label.to_string(),
        };
        let font = if kind == K::Footer {
            self.fonts.small
        } else if tab_active || kind == K::Primary {
            self.fonts.medium
        } else {
            self.fonts.body
        };
        let glyph = control.spec.glyph;
        if label.is_empty() {
            dc.glyph(glyph, center_x, center_y, &self.fonts, foreground);
        } else if matches!(glyph, G::None) {
            let rect = if kind == K::Tab {
                RECT {
                    bottom: rect.bottom - self.px(4),
                    ..rect
                }
            } else {
                rect
            };
            dc.text(&label, rect, font, foreground, DT_CENTER);
        } else if matches!(glyph, G::Chevron) {
            dc.text(
                &label,
                RECT {
                    left: rect.left + self.px(11),
                    right: rect.right - self.px(22),
                    ..rect
                },
                font,
                foreground,
                DT_LEFT,
            );
            dc.glyph(
                glyph,
                rect.right - self.px(15),
                center_y,
                &self.fonts,
                foreground,
            );
        } else {
            let text_width = dc.text_width(&label, font);
            let start = center_x - (text_width + self.px(25)) / 2;
            dc.glyph(glyph, start + self.px(8), center_y, &self.fonts, foreground);
            dc.text(
                &label,
                RECT {
                    left: start + self.px(25),
                    right: rect.right - self.px(3),
                    ..rect
                },
                font,
                foreground,
                DT_LEFT,
            );
        }
        true
    }

    pub fn paint(&self) {
        unsafe {
            let mut info = PAINTSTRUCT::default();
            let client_dc = BeginPaint(self.window, &mut info);
            if self.width > 0 && self.height > 0 && !client_dc.is_null() {
                let buffer = CreateCompatibleDC(client_dc);
                let bitmap = CreateCompatibleBitmap(client_dc, self.width, self.height);
                if !buffer.is_null() && !bitmap.is_null() {
                    let previous = SelectObject(buffer, bitmap);
                    self.paint_chrome(&Draw {
                        dc: buffer,
                        dpi: self.dpi,
                    });
                    let area = info.rcPaint;
                    BitBlt(
                        client_dc,
                        area.left,
                        area.top,
                        area.right - area.left,
                        area.bottom - area.top,
                        buffer,
                        area.left,
                        area.top,
                        SRCCOPY,
                    );
                    SelectObject(buffer, previous);
                } else {
                    self.paint_chrome(&Draw {
                        dc: client_dc,
                        dpi: self.dpi,
                    });
                }
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                if !buffer.is_null() {
                    DeleteDC(buffer);
                }
            }
            EndPaint(self.window, &info);
        }
    }

    fn paint_chrome(&self, dc: &Draw) {
        let w = self.width * 96 / self.dpi as i32;
        let h = self.height * 96 / self.dpi as i32;
        let full = RECT {
            left: 0,
            top: 0,
            right: self.width,
            bottom: self.height,
        };
        dc.fill(full, CANVAS);
        dc.fill(self.rect(0, 0, w + 2, TOP), CHROME);
        dc.fill(self.rect(0, TOP - 1, w + 2, 1), LINE);
        unsafe {
            if !self.icon.is_null() {
                DrawIconEx(
                    dc.dc,
                    self.px(23),
                    self.px(14),
                    self.icon,
                    self.px(38),
                    self.px(38),
                    0,
                    empty(),
                    DI_NORMAL,
                );
            }
        }
        dc.text(
            "WindRu",
            self.rect(71, 11, 99, 44),
            self.fonts.brand,
            INK,
            DT_LEFT,
        );
        dc.fill(self.rect(283, 21, 1, 24), LINE);
        dc.glyph(G::Doc, self.px(305), self.px(32), &self.fonts, ACCENT);
        dc.text(
            &self.name,
            self.rect(321, 10, (w - 585).min(365), 24),
            self.fonts.medium,
            INK,
            DT_LEFT,
        );
        let mut label = if self.modified {
            "Есть изменения"
        } else if self.note.is_empty() {
            "Новый документ"
        } else {
            &self.note
        };
        if self.reading {
            label = "Режим чтения";
        }
        dc.dot(
            self.px(324),
            self.px(44),
            self.px(2),
            if self.modified { 0xb8834f } else { 0x9297b7 },
        );
        dc.text(
            label,
            self.rect(333, 34, 180, 20),
            self.fonts.small,
            MUTED,
            DT_LEFT,
        );
        dc.fill(self.rect(23, 60, w - 46, 1), LINE);
        dc.fill(self.rect(92, 75, 1, 22), LINE);
        dc.rounded(self.rect(24, TOOLBAR, w - 48, 46), 10, WHITE, Some(LINE));
        if self.tab == Tab::Edit {
            for x in [248, 443, 530, 692] {
                dc.fill(self.rect(x, TOOLBAR + 10, 1, 26), LINE);
            }
        }
        if self.show_search {
            dc.rounded(self.rect(24, TOP + 6, 232, 36), 8, WHITE, Some(0xcfcfe5));
            dc.text(
                &self.found,
                self.rect(353, TOP + 4, 225, 39),
                self.fonts.small,
                MUTED,
                DT_LEFT,
            );
        }
        if self.ruler {
            self.draw_ruler(dc);
        }
        let paper = self.paper;
        for (offset, color) in [(4, 0xe9eaf1), (2, 0xe5e6ee)] {
            dc.fill(
                RECT {
                    left: paper.left + self.px(offset),
                    top: paper.top + self.px(offset),
                    right: paper.right + self.px(offset),
                    bottom: paper.bottom + self.px(offset),
                },
                color,
            );
        }
        dc.fill(paper, WHITE);
        dc.border(paper, 0xdedfea);
        let footer = h - FOOTER;
        dc.fill(self.rect(0, footer, w + 2, FOOTER + 2), CHROME);
        dc.fill(self.rect(0, footer, w + 2, 1), LINE);
        dc.glyph(
            G::Doc,
            self.px(33),
            self.px(footer + 18),
            &self.fonts,
            MUTED,
        );
        dc.text(
            &self.count,
            self.rect(48, footer + 1, 260, FOOTER),
            self.fonts.small,
            MUTED,
            DT_LEFT,
        );
        dc.text(
            if self.reading {
                "Только чтение"
            } else {
                "Редактирование"
            },
            self.rect(w - 283, footer + 1, 125, FOOTER),
            self.fonts.small,
            MUTED,
            DT_RIGHT,
        );
        dc.fill(self.rect(w - 151, footer + 10, 1, 17), LINE);
    }

    fn draw_ruler(&self, dc: &Draw) {
        let x1 = self.paper.left;
        let x2 = self.paper.right;
        let y = self.paper.top - self.px(26);
        let ruler = RECT {
            left: x1,
            top: y,
            right: x2,
            bottom: y + self.px(19),
        };
        dc.rounded(ruler, 4, 0xfafbfd, Some(LINE));
        let left = self.editor_area.left;
        let right = self.editor_area.right - self.px(12);
        dc.fill(
            RECT {
                left: left,
                right,
                top: y + 1,
                bottom: ruler.bottom - 1,
            },
            WHITE,
        );
        let printable = right - left;
        for step in 0..=80 {
            let x = left + printable * step / 80;
            let length = if step % 5 == 0 { 6 } else { 3 };
            dc.fill(
                RECT {
                    left: x,
                    top: ruler.bottom - self.px(length) - 1,
                    right: x + self.px(1),
                    bottom: ruler.bottom - 1,
                },
                0xb3b7c8,
            );
            if step > 0 && step % 10 == 0 && step < 80 {
                dc.text(
                    &(step / 5).to_string(),
                    RECT {
                        left: x - self.px(12),
                        right: x + self.px(12),
                        top: y - self.px(1),
                        bottom: y + self.px(13),
                    },
                    self.fonts.small,
                    MUTED,
                    DT_CENTER,
                );
            }
        }
    }
}
