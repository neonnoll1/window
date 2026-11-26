mod paint;

use crate::{
    document::{self, Align, Format, List, Paragraph, Run, Style},
    editor::{self, Editor, Formatting},
};
use paint::{Fonts, Glyph};
use std::{
    cell::RefCell,
    ffi::OsString,
    fs, io, mem,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant},
};
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{LibraryLoader::*, SystemInformation::GetLocalTime},
        UI::{
            Controls::{Dialogs::*, *},
            HiDpi::*,
            Input::KeyboardAndMouse::*,
            Shell::*,
            WindowsAndMessaging::*,
        },
    },
    w,
};

const TITLE: &str = "WindRu";
const SEARCH_ID: usize = 701;
const BUTTON_ID: usize = 1000;
const TIMER: usize = 1;
const REFRESH: u32 = WM_APP + 1;
const KEY_ACTION: u32 = WM_APP + 2;
const INITIAL_WIDTH: i32 = 1220;
const INITIAL_HEIGHT: i32 = 900;
const MIN_WIDTH: i32 = 940;
const MIN_HEIGHT: i32 = 540;
const TOOLBAR: i32 = 112;
const TOP: i32 = 166;
const FOOTER: i32 = 36;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Action {
    File,
    New,
    Open,
    Save,
    SaveAs,
    Print,
    Undo,
    Redo,
    EditTab,
    InsertTab,
    ViewTab,
    Find,
    Face,
    Size,
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
    Heading,
    Symbol,
    Date,
    Rule,
    Ruler,
    Reading,
    ZoomOut,
    ZoomIn,
    ZoomReset,
    FindNext,
    CloseFind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Edit,
    Insert,
    View,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Normal,
    Primary,
    Tab,
    Chip,
    Footer,
}

#[derive(Clone, Copy)]
struct Spec {
    action: Action,
    label: &'static str,
    tooltip: &'static str,
    glyph: Glyph,
    kind: Kind,
}

use Action::*;
use Glyph as G;
use Kind as K;

const SPECS: &[Spec] = &[
    spec(File, "Файл", "", G::Chevron, K::Normal),
    spec(New, "", "Новый документ · Ctrl+N", G::Plus, K::Normal),
    spec(
        Open,
        "Открыть",
        "Открыть документ · Ctrl+O",
        G::Folder,
        K::Normal,
    ),
    spec(
        Save,
        "Сохранить",
        "Сохранить документ · Ctrl+S",
        G::Save,
        K::Primary,
    ),
    spec(Undo, "", "Отменить · Ctrl+Z", G::Undo, K::Normal),
    spec(Redo, "", "Повторить · Ctrl+Y", G::Redo, K::Normal),
    spec(EditTab, "Главная", "", G::None, K::Tab),
    spec(InsertTab, "Вставка", "", G::None, K::Tab),
    spec(ViewTab, "Вид", "", G::None, K::Tab),
    spec(
        Find,
        "Найти",
        "Поиск в документе · Ctrl+F",
        G::Search,
        K::Normal,
    ),
    spec(Face, "", "Шрифт", G::Chevron, K::Chip),
    spec(Size, "", "Размер шрифта", G::Chevron, K::Chip),
    spec(Bold, "", "Полужирный · Ctrl+B", G::Bold, K::Normal),
    spec(Italic, "", "Курсив · Ctrl+I", G::Italic, K::Normal),
    spec(
        Underline,
        "",
        "Подчёркивание · Ctrl+U",
        G::Underline,
        K::Normal,
    ),
    spec(Strike, "", "Зачёркивание", G::Strike, K::Normal),
    spec(Color, "", "Цвет текста", G::Color, K::Normal),
    spec(Left, "", "По левому краю", G::Left, K::Normal),
    spec(Center, "", "По центру", G::Center, K::Normal),
    spec(Right, "", "По правому краю", G::Right, K::Normal),
    spec(Justify, "", "По ширине", G::Justify, K::Normal),
    spec(Bullets, "", "Маркированный список", G::Bullets, K::Normal),
    spec(Numbers, "", "Нумерованный список", G::Numbers, K::Normal),
    spec(
        Heading,
        "Стиль текста",
        "Заголовок или обычный текст",
        G::Chevron,
        K::Chip,
    ),
    spec(Symbol, "Символ", "Вставить символ", G::Symbol, K::Chip),
    spec(Date, "Дата", "Вставить текущую дату", G::Calendar, K::Chip),
    spec(
        Rule,
        "Разделитель",
        "Вставить текстовый разделитель",
        G::Minus,
        K::Chip,
    ),
    spec(
        Ruler,
        "Линейка",
        "Показать или скрыть линейку",
        G::Ruler,
        K::Chip,
    ),
    spec(
        Reading,
        "Режим чтения",
        "Защитить текст от случайного редактирования",
        G::Book,
        K::Chip,
    ),
    spec(ZoomOut, "", "Уменьшить масштаб", G::Minus, K::Footer),
    spec(ZoomIn, "", "Увеличить масштаб", G::Plus, K::Footer),
    spec(ZoomReset, "", "Вернуть масштаб 100 %", G::None, K::Footer),
    spec(
        FindNext,
        "Далее",
        "Найти следующее совпадение · Enter",
        G::None,
        K::Chip,
    ),
    spec(CloseFind, "", "Закрыть поиск · Esc", G::Close, K::Normal),
];

const fn spec(
    action: Action,
    label: &'static str,
    tooltip: &'static str,
    glyph: Glyph,
    kind: Kind,
) -> Spec {
    Spec {
        action,
        label,
        tooltip,
        glyph,
        kind,
    }
}

struct Control {
    spec: Spec,
    hwnd: HWND,
    area: RECT,
    shown: bool,
    hover: u8,
    _tip: Vec<u16>,
}

struct App {
    window: HWND,
    editor: Editor,
    search: HWND,
    buttons: Vec<Control>,
    fonts: Fonts,
    icon: HICON,
    dpi: u32,
    width: i32,
    height: i32,
    paper: RECT,
    editor_area: RECT,
    path: Option<PathBuf>,
    name: String,
    tab: Tab,
    format: Formatting,
    modified: bool,
    undo_available: bool,
    redo_available: bool,
    ruler: bool,
    reading: bool,
    zoom: i32,
    show_search: bool,
    has_styles: bool,
    note: String,
    count: String,
    found: String,
    recount: Option<Instant>,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn empty() -> *mut core::ffi::c_void {
    ptr::null_mut()
}

pub fn start() {
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        if LoadLibraryExW(w!("msftedit.dll"), empty(), LOAD_LIBRARY_SEARCH_SYSTEM32).is_null() {
            MessageBoxW(
                empty(),
                w!("Не удалось запустить системный редактор Windows."),
                w!("WindRu"),
                MB_ICONERROR,
            );
            return;
        }
        let init = INITCOMMONCONTROLSEX {
            dwSize: mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_WIN95_CLASSES,
        };
        InitCommonControlsEx(&init);
        let class = w!("WindRuEditorWindow");
        let window_class = WNDCLASSEXW {
            cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
            hInstance: instance,
            hCursor: LoadCursorW(empty(), IDC_ARROW),
            hIcon: LoadIconW(instance, 1usize as *const u16),
            hIconSm: LoadIconW(instance, 1usize as *const u16),
            lpszClassName: class,
            lpfnWndProc: Some(window_proc),
            style: CS_HREDRAW | CS_VREDRAW,
            ..Default::default()
        };
        if RegisterClassExW(&window_class) == 0 {
            return;
        }
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: INITIAL_WIDTH,
            bottom: INITIAL_HEIGHT,
        };
        let dpi = GetDpiForSystem().max(96);
        bounds.right = INITIAL_WIDTH * dpi as i32 / 96;
        bounds.bottom = INITIAL_HEIGHT * dpi as i32 / 96;
        AdjustWindowRectExForDpi(&mut bounds, WS_OVERLAPPEDWINDOW, 0, 0, dpi);
        let mut work = RECT::default();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut _ as _, 0);
        let avail_w = work.right - work.left;
        let avail_h = work.bottom - work.top;
        let width = (bounds.right - bounds.left).min(avail_w - 24).max(480);
        let height = (bounds.bottom - bounds.top).min(avail_h - 30).max(390);
        let window = CreateWindowExW(
            0,
            class,
            w!("WindRu"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            work.left + (avail_w - width) / 2,
            work.top + (avail_h - height) / 2,
            width,
            height,
            empty(),
            empty(),
            instance,
            ptr::null(),
        );
        if window.is_null() {
            return;
        }
        if let Some(path) = std::env::args_os().nth(1) {
            if let Some(app) = app_state(window) {
                if let Ok(mut app) = app.try_borrow_mut() {
                    app.open_path(PathBuf::from(path));
                }
            }
        }
        ShowWindow(window, SW_SHOW);
        UpdateWindow(window);
        let mut message = MSG::default();
        while GetMessageW(&mut message, empty(), 0, 0) > 0 {
            if message.message == WM_KEYDOWN {
                if let Some(app) = app_state(window) {
                    if let Ok(mut app) = app.try_borrow_mut() {
                        if app.keyboard(message.wParam as u16) {
                            continue;
                        }
                    }
                }
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn app_state(window: HWND) -> Option<&'static RefCell<App>> {
    unsafe { (GetWindowLongPtrW(window, GWLP_USERDATA) as *const RefCell<App>).as_ref() }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_CREATE {
            let Some(app) = App::new(window) else {
                return -1;
            };
            SetWindowLongPtrW(
                window,
                GWLP_USERDATA,
                Box::into_raw(Box::new(RefCell::new(app))) as isize,
            );
            DragAcceptFiles(window, 1);
            SetTimer(window, TIMER, 16, None);
            return 0;
        }
        if message == WM_NCDESTROY {
            let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut RefCell<App>;
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            if !state.is_null() {
                drop(Box::from_raw(state));
            }
            return DefWindowProcW(window, message, wparam, lparam);
        }
        if message == WM_DESTROY {
            KillTimer(window, TIMER);
            PostQuitMessage(0);
            return 0;
        }
        let Some(state) = app_state(window) else {
            return DefWindowProcW(window, message, wparam, lparam);
        };
        if message == WM_DRAWITEM {
            if let Ok(app) = state.try_borrow() {
                let draw = &*(lparam as *const DRAWITEMSTRUCT);
                if app.draw_control(draw) {
                    return 1;
                }
            }
            return 0;
        }
        if message == WM_CLOSE {
            let may_close = state
                .try_borrow_mut()
                .is_ok_and(|mut app| app.confirm_change());
            if may_close {
                DestroyWindow(window);
            }
            return 0;
        }
        let Ok(mut app) = state.try_borrow_mut() else {
            return DefWindowProcW(window, message, wparam, lparam);
        };
        match message {
            WM_SIZE => {
                app.layout();
                0
            }
            WM_DPICHANGED => {
                app.dpi = (wparam & 0xffff) as u32;
                app.reload_fonts();
                let rect = &*(lparam as *const RECT);
                SetWindowPos(
                    window,
                    empty(),
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                app.layout();
                0
            }
            WM_GETMINMAXINFO => {
                let limits = &mut *(lparam as *mut MINMAXINFO);
                let mut minimum = RECT {
                    left: 0,
                    top: 0,
                    right: app.px(MIN_WIDTH),
                    bottom: app.px(MIN_HEIGHT),
                };
                AdjustWindowRectExForDpi(&mut minimum, WS_OVERLAPPEDWINDOW, 0, 0, app.dpi);
                limits.ptMinTrackSize.x = minimum.right - minimum.left;
                limits.ptMinTrackSize.y = minimum.bottom - minimum.top;
                0
            }
            WM_SETFOCUS => {
                SetFocus(app.editor.0);
                0
            }
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                app.paint();
                0
            }
            WM_TIMER if wparam == TIMER => {
                app.tick();
                0
            }
            WM_COMMAND => {
                let id = wparam & 0xffff;
                let event = ((wparam >> 16) & 0xffff) as u32;
                if event == EN_CHANGE && id == editor::EDITOR_ID {
                    app.schedule_count();
                    PostMessageW(window, REFRESH, 0, 0);
                } else if event == EN_CHANGE && id == SEARCH_ID {
                    app.found.clear();
                    app.redraw();
                } else if event == BN_CLICKED
                    && let Some(button) = id
                        .checked_sub(BUTTON_ID)
                        .and_then(|index| app.buttons.get(index))
                {
                    let action = button.spec.action;
                    app.action(action);
                }
                0
            }
            WM_NOTIFY if lparam != 0 => {
                let header = &*(lparam as *const NMHDR);
                if header.idFrom == editor::EDITOR_ID && header.code == editor::SELECTION_EVENT {
                    PostMessageW(window, REFRESH, 0, 0);
                }
                0
            }
            REFRESH => {
                app.refresh();
                0
            }
            KEY_ACTION => {
                if let Some(action) = [
                    New, Open, Save, SaveAs, Print, Undo, Redo, Bold, Italic, Underline, Find,
                    FindNext, CloseFind, ZoomIn, ZoomOut, ZoomReset,
                ]
                .get(wparam)
                {
                    app.action(*action);
                }
                0
            }
            WM_LBUTTONDOWN => {
                SetFocus(app.editor.0);
                0
            }
            WM_DROPFILES => {
                let drop = wparam as HDROP;
                let size = DragQueryFileW(drop, 0, ptr::null_mut(), 0);
                let mut value = vec![0u16; size as usize + 1];
                DragQueryFileW(drop, 0, value.as_mut_ptr(), value.len() as u32);
                DragFinish(drop);
                if size > 0 {
                    app.open_path(PathBuf::from(OsString::from_wide(&value[..size as usize])));
                }
                0
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

impl App {
    fn new(window: HWND) -> Option<Self> {
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            let dpi = GetDpiForWindow(window).max(96);
            let icon = LoadIconW(instance, 1usize as *const u16);
            let editor = Editor::create(window);
            if editor.0.is_null() {
                return None;
            }
            let search = CreateWindowExW(
                0,
                w!("EDIT"),
                ptr::null(),
                WS_CHILD | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                0,
                0,
                0,
                0,
                window,
                SEARCH_ID as _,
                instance,
                ptr::null(),
            );
            SendMessageW(search, EM_SETCUEBANNER, 1, w!("Текст для поиска") as isize);
            SendMessageW(search, EM_SETLIMITTEXT, 512, 0);
            let tips = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                TOOLTIPS_CLASSW,
                ptr::null(),
                WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
                0,
                0,
                0,
                0,
                window,
                empty(),
                instance,
                ptr::null(),
            );
            SendMessageW(tips, TTM_SETDELAYTIME, TTDT_INITIAL as usize, 480);
            let buttons = SPECS
                .iter()
                .enumerate()
                .map(|(index, spec)| {
                    let name = wide(if spec.label.is_empty() {
                        spec.tooltip
                    } else {
                        spec.label
                    });
                    let hwnd = CreateWindowExW(
                        0,
                        w!("BUTTON"),
                        name.as_ptr(),
                        WS_CHILD | WS_TABSTOP | BS_OWNERDRAW as u32,
                        0,
                        0,
                        0,
                        0,
                        window,
                        (BUTTON_ID + index) as _,
                        instance,
                        ptr::null(),
                    );
                    let mut tip = wide(spec.tooltip);
                    if !spec.tooltip.is_empty() {
                        let info = TTTOOLINFOW {
                            cbSize: (mem::size_of::<TTTOOLINFOW>() - mem::size_of::<usize>())
                                as u32,
                            hwnd: window,
                            uId: hwnd as usize,
                            uFlags: TTF_IDISHWND | TTF_SUBCLASS,
                            lpszText: tip.as_mut_ptr(),
                            ..Default::default()
                        };
                        SendMessageW(tips, TTM_ADDTOOLW, 0, &info as *const _ as isize);
                    }
                    Control {
                        spec: *spec,
                        hwnd,
                        area: RECT::default(),
                        shown: false,
                        hover: 0,
                        _tip: tip,
                    }
                })
                .collect();
            let mut app = Self {
                window,
                editor,
                search,
                buttons,
                fonts: Fonts::new(dpi),
                icon,
                dpi,
                width: 0,
                height: 0,
                paper: RECT::default(),
                editor_area: RECT::default(),
                path: None,
                name: String::from("Знакомство с WindRu"),
                tab: Tab::Edit,
                format: Formatting::default(),
                modified: false,
                undo_available: false,
                redo_available: false,
                ruler: true,
                reading: false,
                zoom: 100,
                show_search: false,
                has_styles: true,
                note: String::from("Пример документа"),
                count: String::new(),
                found: String::new(),
                recount: None,
            };
            if editor.load_rtf(&document::to_rtf(&welcome())).is_err() {
                editor.text("Добро пожаловать в WindRu");
            }
            app.reload_search_font();
            app.refresh();
            app.update_count();
            app.layout();
            Some(app)
        }
    }

    fn px(&self, value: i32) -> i32 {
        (value * self.dpi as i32 + 48) / 96
    }

    fn reload_fonts(&mut self) {
        let old = mem::replace(&mut self.fonts, Fonts::new(self.dpi));
        self.reload_search_font();
        drop(old);
    }

    fn reload_search_font(&self) {
        unsafe { SendMessageW(self.search, WM_SETFONT, self.fonts.body as usize, 1) };
    }

    fn layout(&mut self) {
        unsafe {
            let mut client = RECT::default();
            GetClientRect(self.window, &mut client);
            self.width = client.right;
            self.height = client.bottom;
            let w = self.width * 96 / self.dpi as i32;
            let h = self.height * 96 / self.dpi as i32;
            let paper_width = (766 * self.zoom / 100).clamp(470, (w - 72).max(470));
            let paper_left = (w - paper_width) / 2;
            let canvas_top = TOP + if self.show_search { 48 } else { 0 };
            let paper_top = canvas_top + if self.ruler { 43 } else { 28 };
            let paper_bottom = (h - FOOTER - 24).max(paper_top + 110);
            let margin = (68 * self.zoom / 100).clamp(38, paper_width / 7);
            self.paper = self.rect(paper_left, paper_top, paper_width, paper_bottom - paper_top);
            self.editor_area = self.rect(
                paper_left + margin,
                paper_top + (55 * self.zoom / 100).clamp(30, 78),
                paper_width - margin * 2 + 12,
                (paper_bottom - paper_top - (55 * self.zoom / 100).clamp(30, 78) - 30).max(40),
            );
            self.position(self.editor.0, self.editor_area);
            let mut placements = vec![
                (File, 24, 70, 68, 31),
                (EditTab, 99, 66, 95, 39),
                (InsertTab, 201, 66, 93, 39),
                (ViewTab, 301, 66, 60, 39),
                (Find, w - 112, 69, 86, 31),
                (New, 172, 17, 32, 32),
                (Undo, 210, 17, 31, 32),
                (Redo, 242, 17, 31, 32),
                (Open, w - 254, 15, 103, 36),
                (Save, w - 141, 15, 117, 36),
                (ZoomOut, w - 141, h - 30, 27, 25),
                (ZoomReset, w - 111, h - 30, 56, 25),
                (ZoomIn, w - 52, h - 30, 27, 25),
            ];
            let row = TOOLBAR + 6;
            match self.tab {
                Tab::Edit => {
                    placements.extend([
                        (Face, 31, row, 149, 34),
                        (Size, 187, row, 55, 34),
                        (Bold, 255, row, 34, 34),
                        (Italic, 292, row, 34, 34),
                        (Underline, 329, row, 34, 34),
                        (Strike, 366, row, 34, 34),
                        (Color, 402, row, 34, 34),
                        (Bullets, 452, row, 34, 34),
                        (Numbers, 489, row, 34, 34),
                        (Left, 539, row, 34, 34),
                        (Center, 576, row, 34, 34),
                        (Right, 613, row, 34, 34),
                        (Justify, 650, row, 34, 34),
                        (Heading, 700, row, 154, 34),
                    ]);
                }
                Tab::Insert => placements.extend([
                    (Symbol, 32, row, 108, 34),
                    (Date, 148, row, 91, 34),
                    (Rule, 247, row, 132, 34),
                ]),
                Tab::View => {
                    placements.extend([(Ruler, 32, row, 111, 34), (Reading, 152, row, 157, 34)])
                }
            }
            if self.show_search {
                let field = self.rect(39, TOP + 13, 208, 22);
                self.position(self.search, field);
                placements.extend([
                    (FindNext, 260, TOP + 7, 78, 34),
                    (CloseFind, w - 63, TOP + 7, 34, 34),
                ]);
            }
            ShowWindow(
                self.search,
                if self.show_search { SW_SHOWNA } else { SW_HIDE },
            );
            for index in 0..self.buttons.len() {
                let button = &self.buttons[index];
                let area = placements
                    .iter()
                    .find(|row| row.0 == button.spec.action)
                    .map(|&(_, x, y, width, height)| self.rect(x, y, width, height));
                let hwnd = button.hwnd;
                if let Some(area) = area {
                    self.position(hwnd, area);
                }
                ShowWindow(hwnd, if area.is_some() { SW_SHOWNA } else { SW_HIDE });
                let button = &mut self.buttons[index];
                button.shown = area.is_some();
                button.area = area.unwrap_or_default();
            }
            self.redraw();
            self.redraw_buttons();
        }
    }

    fn rect(&self, x: i32, y: i32, width: i32, height: i32) -> RECT {
        RECT {
            left: self.px(x),
            top: self.px(y),
            right: self.px(x + width),
            bottom: self.px(y + height),
        }
    }

    fn position(&self, hwnd: HWND, area: RECT) {
        unsafe {
            SetWindowPos(
                hwnd,
                empty(),
                area.left,
                area.top,
                area.right - area.left,
                area.bottom - area.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    fn action(&mut self, action: Action) {
        match action {
            File => {
                let labels = [
                    "Новый документ\tCtrl+N",
                    "Открыть…\tCtrl+O",
                    "Сохранить\tCtrl+S",
                    "Сохранить как…\tCtrl+Shift+S",
                    "Печать…\tCtrl+P",
                ];
                if let Some(index) = self.menu(File, &labels) {
                    self.action([New, Open, Save, SaveAs, Print][index]);
                    return;
                }
            }
            New => {
                if self.confirm_change() {
                    self.editor.text("");
                    self.reset_document(None, "Без названия".into(), "", false);
                }
            }
            Open => {
                if let Some((path, _)) = self.file_dialog(false) {
                    self.open_path(path);
                }
            }
            Save => {
                self.save(false);
            }
            SaveAs => {
                self.save(true);
            }
            Print => {
                if let Err(error) = self.editor.print(self.window, &self.name) {
                    self.error("Печать", &error.to_string());
                }
            }
            Undo if !self.reading => self.editor.undo(),
            Redo if !self.reading => self.editor.redo(),
            EditTab | InsertTab | ViewTab => {
                self.tab = match action {
                    EditTab => Tab::Edit,
                    InsertTab => Tab::Insert,
                    _ => Tab::View,
                };
                self.layout();
            }
            Find => {
                if !self.show_search {
                    self.show_search = true;
                    self.layout();
                }
                unsafe {
                    SetFocus(self.search);
                    SendMessageW(self.search, EM_SETSEL, 0, -1);
                }
                return;
            }
            CloseFind => {
                self.show_search = false;
                self.found.clear();
                self.layout();
            }
            FindNext => {
                self.search_next();
                return;
            }
            Ruler => {
                self.ruler = !self.ruler;
                self.layout();
            }
            Reading => {
                self.reading = !self.reading;
                unsafe {
                    SendMessageW(self.editor.0, EM_SETREADONLY, usize::from(self.reading), 0);
                }
            }
            ZoomOut | ZoomIn | ZoomReset => {
                self.zoom = match action {
                    ZoomOut => (self.zoom - 10).max(50),
                    ZoomIn => (self.zoom + 10).min(180),
                    _ => 100,
                };
                self.editor.zoom(self.zoom);
                self.layout();
            }
            _ if !self.reading => {
                self.format_action(action);
            }
            _ => {}
        }
        unsafe {
            SetFocus(self.editor.0);
        }
        self.schedule_count();
        self.refresh();
    }

    fn format_action(&mut self, action: Action) {
        let styled = match action {
            Bold => {
                self.editor.toggle(editor::BOLD);
                true
            }
            Italic => {
                self.editor.toggle(editor::ITALIC);
                true
            }
            Underline => {
                self.editor.toggle(editor::UNDERLINE);
                true
            }
            Strike => {
                self.editor.toggle(editor::STRIKE);
                true
            }
            Left | Center | Right | Justify => {
                self.editor.align(match action {
                    Center => Align::Center,
                    Right => Align::Right,
                    Justify => Align::Justify,
                    _ => Align::Left,
                });
                true
            }
            Bullets | Numbers => {
                self.editor.list(if action == Bullets {
                    List::Bullet
                } else {
                    List::Number
                });
                true
            }
            Face => {
                let choices = [
                    "Segoe UI",
                    "Arial",
                    "Calibri",
                    "Cambria",
                    "Georgia",
                    "Times New Roman",
                    "Consolas",
                ];
                self.menu(Face, &choices)
                    .map(|index| self.editor.set_font(choices[index]))
                    .is_some()
            }
            Size => {
                let labels = [
                    "8", "9", "10", "11", "12", "14", "16", "18", "22", "26", "32", "40", "48",
                    "64", "72",
                ];
                self.menu(Size, &labels)
                    .map(|index| self.editor.set_size(labels[index].parse().unwrap_or(12)))
                    .is_some()
            }
            Color => {
                let choices = [
                    "Графитовый",
                    "Чёрный",
                    "Серый",
                    "Индиго",
                    "Синий",
                    "Зелёный",
                    "Бордовый",
                ];
                let colors = [
                    0x273039, 0x111111, 0x666b79, 0x4b468e, 0x1460a0, 0x28735e, 0x9c4050,
                ];
                self.menu(Color, &choices)
                    .map(|index| self.editor.set_color(colors[index]))
                    .is_some()
            }
            Heading => {
                let choices = ["Обычный текст", "Заголовок", "Подзаголовок"];
                self.menu(Heading, &choices)
                    .map(|index| self.editor.heading([12, 28, 17][index], index > 0))
                    .is_some()
            }
            Symbol => {
                let choices = [
                    "—    Тире",
                    "«    Левая кавычка",
                    "»    Правая кавычка",
                    "…    Многоточие",
                    "•    Маркер",
                    "№    Номер",
                    "©    Знак авторства",
                    "→    Стрелка",
                ];
                if let Some(index) = self.menu(Symbol, &choices) {
                    self.editor
                        .insert(["—", "«", "»", "…", "•", "№", "©", "→"][index]);
                }
                false
            }
            Rule => {
                self.editor.insert("\r────────────\r");
                false
            }
            Date => {
                let mut now = SYSTEMTIME::default();
                unsafe {
                    GetLocalTime(&mut now);
                }
                self.editor.insert(&format!(
                    "{:02}.{:02}.{:04}",
                    now.wDay, now.wMonth, now.wYear
                ));
                false
            }
            _ => false,
        };
        self.has_styles |= styled;
    }

    fn menu(&self, action: Action, entries: &[&str]) -> Option<usize> {
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return None;
            }
            for (index, label) in entries.iter().enumerate() {
                let value = wide(label);
                AppendMenuW(menu, MF_STRING, index + 1, value.as_ptr());
            }
            let area = self
                .buttons
                .iter()
                .find(|button| button.spec.action == action)?
                .area;
            let mut point = POINT {
                x: area.left,
                y: area.bottom + self.px(3),
            };
            ClientToScreen(self.window, &mut point);
            let choice = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_LEFTALIGN | TPM_TOPALIGN,
                point.x,
                point.y,
                0,
                self.window,
                ptr::null(),
            );
            DestroyMenu(menu);
            (choice as usize).checked_sub(1)
        }
    }

    fn keyboard(&mut self, key: u16) -> bool {
        unsafe {
            let control = GetKeyState(VK_CONTROL as i32) < 0;
            let shift = GetKeyState(VK_SHIFT as i32) < 0;
            let focus = GetFocus();
            let action = if control {
                match key {
                    0x4e => Some(0),
                    0x4f => Some(1),
                    0x53 if shift => Some(3),
                    0x53 => Some(2),
                    0x50 => Some(4),
                    0x5a if focus != self.search && shift => Some(6),
                    0x5a if focus != self.search => Some(5),
                    0x59 if focus != self.search => Some(6),
                    0x42 if focus != self.search => Some(7),
                    0x49 if focus != self.search => Some(8),
                    0x55 if focus != self.search => Some(9),
                    0x46 => Some(10),
                    VK_ADD | VK_OEM_PLUS => Some(13),
                    VK_SUBTRACT | VK_OEM_MINUS => Some(14),
                    0x30 => Some(15),
                    0x41 if focus == self.editor.0 => {
                        self.editor.select_all();
                        return true;
                    }
                    _ => None,
                }
            } else {
                match key {
                    VK_RETURN if focus == self.search => Some(11),
                    VK_F3 if self.show_search => Some(11),
                    VK_ESCAPE if self.show_search => Some(12),
                    VK_F6 => {
                        if focus == self.editor.0 {
                            SetFocus(self.buttons[0].hwnd);
                        } else {
                            SetFocus(self.editor.0);
                        }
                        return true;
                    }
                    VK_TAB if focus != self.editor.0 => {
                        let controls = self
                            .buttons
                            .iter()
                            .filter(|button| button.shown)
                            .map(|button| button.hwnd)
                            .chain(self.show_search.then_some(self.search))
                            .collect::<Vec<_>>();
                        let index = controls.iter().position(|hwnd| *hwnd == focus);
                        let next = match (index, shift) {
                            (Some(i), true) => (i + controls.len() - 1) % controls.len(),
                            (Some(i), false) => (i + 1) % controls.len(),
                            (None, _) => 0,
                        };
                        if let Some(&next) = controls.get(next) {
                            SetFocus(next);
                        }
                        return true;
                    }
                    _ => None,
                }
            };
            if let Some(index) = action {
                PostMessageW(self.window, KEY_ACTION, index, 0);
                return true;
            }
        }
        false
    }

    fn search_next(&mut self) {
        unsafe {
            let mut text = [0u16; 513];
            let length =
                GetWindowTextW(self.search, text.as_mut_ptr(), text.len() as i32).max(0) as usize;
            let query = String::from_utf16_lossy(&text[..length]);
            self.found = if query.is_empty() {
                "Введите текст".into()
            } else if self.editor.find(&query) {
                "Найдено".into()
            } else {
                "Совпадений нет".into()
            };
            SetFocus(self.search);
            self.redraw();
        }
    }

    fn confirm_change(&mut self) -> bool {
        if !self.editor.modified() {
            return true;
        }
        let question = wide(&format!("Сохранить изменения в документе «{}»?", self.name));
        let answer = unsafe {
            MessageBoxW(
                self.window,
                question.as_ptr(),
                w!("WindRu"),
                MB_YESNOCANCEL | MB_ICONQUESTION,
            )
        };
        answer == IDNO || (answer == IDYES && self.save(false))
    }

    fn open_path(&mut self, path: PathBuf) {
        if !self.confirm_change() {
            return;
        }
        let result: Result<_, Box<dyn std::error::Error>> = (|| {
            let format = Format::detect(&path).ok_or("Выберите файл RTF, TXT или DOCX")?;
            if fs::metadata(&path)?.len() > document::MAX_FILE {
                return Err("Документ слишком большой".into());
            }
            match format {
                Format::Docx => {
                    let paragraphs = document::open_docx(&path)?;
                    self.replace_rtf(&document::to_rtf(&paragraphs))?;
                    self.editor.restore_docx_lists(&paragraphs);
                }
                Format::Rtf => {
                    let bytes = fs::read(&path)?;
                    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
                    let start = bytes
                        .iter()
                        .position(|byte| !byte.is_ascii_whitespace())
                        .unwrap_or(bytes.len());
                    self.replace_rtf(&bytes[start..])?;
                }
                Format::Txt => self.editor.text(&document::decode_text(&fs::read(&path)?)?),
            }
            Ok(format)
        })();
        match result {
            Ok(format) => {
                let name = document_name(&path);
                let note = if format == Format::Docx {
                    "Копия DOCX"
                } else {
                    "Открыт"
                };
                self.reset_document(
                    (format != Format::Docx).then_some(path),
                    name,
                    note,
                    format != Format::Txt,
                );
                unsafe {
                    SetFocus(self.editor.0);
                }
            }
            Err(error) => self.error("Не удалось открыть документ", &error.to_string()),
        }
    }

    fn replace_rtf(&mut self, bytes: &[u8]) -> io::Result<()> {
        let checkpoint = self.editor.save_rtf()?;
        let was_dirty = self.editor.modified();
        let result = self.editor.load_rtf(bytes);
        if result.is_err() && bytes.starts_with(b"{\\rtf") {
            let _ = self.editor.load_rtf(&checkpoint);
            unsafe {
                SendMessageW(self.editor.0, EM_SETMODIFY, usize::from(was_dirty), 0);
            }
        }
        result
    }

    fn reset_document(&mut self, path: Option<PathBuf>, name: String, note: &str, styled: bool) {
        self.path = path;
        self.name = name;
        self.note = note.into();
        self.has_styles = styled;
        self.reading = false;
        self.show_search = false;
        self.found.clear();
        self.editor.clean();
        unsafe {
            SendMessageW(self.editor.0, EM_SETREADONLY, 0, 0);
        }
        self.update_count();
        self.refresh();
        self.layout();
    }

    fn save(&mut self, choose: bool) -> bool {
        let existing_format = self.path.as_deref().and_then(Format::detect);
        let ask_path = choose
            || self.path.is_none()
            || (self.has_styles && existing_format == Some(Format::Txt));
        let (path, format) = if ask_path {
            let Some(value) = self.file_dialog(true) else {
                return false;
            };
            value
        } else {
            (self.path.clone().unwrap(), existing_format.unwrap())
        };
        if format == Format::Txt && self.has_styles {
            let answer = unsafe {
                MessageBoxW(
                    self.window,
                    w!("В формате TXT сохранится только текст. Сохранить файл TXT?"),
                    w!("WindRu"),
                    MB_OKCANCEL | MB_ICONQUESTION,
                )
            };
            if answer != IDOK {
                return false;
            }
        }
        let result: Result<(), Box<dyn std::error::Error>> = (|| {
            let bytes = match format {
                Format::Rtf => self.editor.save_rtf()?,
                Format::Txt => self.editor.plain_text(true).into_bytes(),
                Format::Docx => document::write_docx(&self.editor.paragraphs())?,
            };
            document::save(&path, &bytes)?;
            Ok(())
        })();
        if let Err(error) = result {
            self.error("Не удалось сохранить документ", &error.to_string());
            return false;
        }
        self.name = document_name(&path);
        self.path = Some(path);
        self.note = "Сохранено".into();
        self.editor.clean();
        self.refresh();
        true
    }

    fn file_dialog(&self, save: bool) -> Option<(PathBuf, Format)> {
        unsafe {
            let mut buffer = vec![0u16; 32768];
            if save {
                let displayed_path = Path::new(&self.name);
                let copy_name = (self.path.is_none()
                    && Format::detect(displayed_path) == Some(Format::Docx))
                .then(|| {
                    let stem = displayed_path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or(&self.name);
                    format!("{stem} — копия")
                });
                let name = self
                    .path
                    .as_deref()
                    .and_then(Path::file_stem)
                    .and_then(|stem| stem.to_str())
                    .or(copy_name.as_deref())
                    .unwrap_or(&self.name);
                let utf16: Vec<u16> = name.encode_utf16().take(300).collect();
                buffer[..utf16.len()].copy_from_slice(&utf16);
            }
            let open_filters =
                w!("Документы (*.rtf; *.docx; *.txt)\0*.rtf;*.docx;*.txt\0Все файлы\0*.*\0");
            let save_filters = w!(
                "Форматированный текст (*.rtf)\0*.rtf\0Документ (*.docx)\0*.docx\0Обычный текст (*.txt)\0*.txt\0"
            );
            let mut dialog = OPENFILENAMEW {
                lStructSize: mem::size_of::<OPENFILENAMEW>() as u32,
                hwndOwner: self.window,
                lpstrFilter: if save { save_filters } else { open_filters },
                nFilterIndex: if save && !self.has_styles {
                    match self.path.as_deref().and_then(Format::detect) {
                        Some(Format::Txt) => 3,
                        _ => 1,
                    }
                } else if save
                    && self.path.as_deref().and_then(Format::detect) == Some(Format::Docx)
                {
                    2
                } else {
                    1
                },
                lpstrFile: buffer.as_mut_ptr(),
                nMaxFile: buffer.len() as u32,
                lpstrTitle: if save {
                    w!("Сохранить документ")
                } else {
                    w!("Открыть документ")
                },
                lpstrDefExt: ptr::null(),
                Flags: OFN_EXPLORER
                    | OFN_NOCHANGEDIR
                    | OFN_PATHMUSTEXIST
                    | OFN_HIDEREADONLY
                    | if save {
                        OFN_OVERWRITEPROMPT
                    } else {
                        OFN_FILEMUSTEXIST
                    },
                ..Default::default()
            };
            let result = if save {
                GetSaveFileNameW(&mut dialog)
            } else {
                GetOpenFileNameW(&mut dialog)
            };
            if result == 0 {
                let error = CommDlgExtendedError();
                if error != 0 {
                    self.error("Ошибка диалога", &format!("Системный код {error:#x}"));
                }
                return None;
            }
            let length = buffer
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(buffer.len());
            let mut path = PathBuf::from(OsString::from_wide(&buffer[..length]));
            if save && path.extension().is_none_or(|ext| ext.is_empty()) {
                path.set_extension(match dialog.nFilterIndex {
                    2 => "docx",
                    3 => "txt",
                    _ => "rtf",
                });
                if path.exists() {
                    let question = wide(&format!("Заменить файл «{}»?", document_name(&path)));
                    if MessageBoxW(
                        self.window,
                        question.as_ptr(),
                        w!("Сохранить документ"),
                        MB_YESNO | MB_ICONQUESTION,
                    ) != IDYES
                    {
                        return None;
                    }
                }
            }
            match Format::detect(&path) {
                Some(format) => Some((path, format)),
                None => {
                    self.error("Формат файла", "Допустимые расширения — RTF, DOCX и TXT.");
                    None
                }
            }
        }
    }

    fn error(&self, title: &str, error: &str) {
        let title = wide(title);
        let message = wide(error);
        unsafe {
            MessageBoxW(
                self.window,
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    fn schedule_count(&mut self) {
        self.recount = Some(Instant::now() + Duration::from_millis(145));
    }

    fn update_count(&mut self) {
        self.recount = None;
        let text = self.editor.plain_text(false);
        let count = text
            .split_whitespace()
            .filter(|word| word.chars().any(char::is_alphanumeric))
            .count();
        let chars = text.chars().count();
        self.count = format!(
            "{} {}   ·   {} {}",
            count,
            plural(count, "слово", "слова", "слов"),
            chars,
            plural(chars, "символ", "символа", "символов")
        );
        self.redraw();
    }

    fn refresh(&mut self) {
        self.format = self.editor.formatting();
        let modified = self.editor.modified();
        let undo = self.editor.can_undo() && !self.reading;
        let redo = self.editor.can_redo() && !self.reading;
        if undo != self.undo_available || redo != self.redo_available {
            self.undo_available = undo;
            self.redo_available = redo;
        }
        self.modified = modified;
        let title = wide(&format!(
            "{}{}  ·  {TITLE}",
            self.name,
            if modified { " *" } else { "" }
        ));
        unsafe {
            SetWindowTextW(self.window, title.as_ptr());
        }
        self.redraw();
        self.redraw_buttons();
    }

    fn redraw(&self) {
        unsafe {
            InvalidateRect(self.window, ptr::null(), 0);
        }
    }

    fn redraw_buttons(&self) {
        for button in &self.buttons {
            if button.shown {
                unsafe {
                    InvalidateRect(button.hwnd, ptr::null(), 0);
                }
            }
        }
    }

    fn tick(&mut self) {
        if self.recount.is_some_and(|until| Instant::now() >= until) {
            self.update_count();
        }
        unsafe {
            let mut point = POINT::default();
            GetCursorPos(&mut point);
            let hovered = WindowFromPoint(point);
            for button in self.buttons.iter_mut().filter(|button| button.shown) {
                let next = if button.hwnd == hovered {
                    button.hover.saturating_add(34)
                } else {
                    button.hover.saturating_sub(34)
                };
                if next != button.hover {
                    button.hover = next;
                    InvalidateRect(button.hwnd, ptr::null(), 0);
                }
            }
        }
    }
}

fn document_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || "Без названия".into(),
        |file| file.to_string_lossy().into_owned(),
    )
}

fn plural(value: usize, one: &'static str, few: &'static str, other: &'static str) -> &'static str {
    match (value % 100, value % 10) {
        (11..=14, _) => other,
        (_, 1) => one,
        (_, 2..=4) => few,
        _ => other,
    }
}

fn welcome() -> Vec<Paragraph> {
    let text = |text: &str, size: u16, color: u32, bold| Paragraph {
        runs: vec![Run {
            text: text.into(),
            style: Style {
                points: Some(size),
                color: Some(color),
                bold,
                ..Default::default()
            },
        }],
        ..Default::default()
    };
    vec![
        text("Добро пожаловать в WindRu", 52, 0x242538, true),
        text(
            "Откройте свой файл или начните с чистого листа.",
            30,
            0x64677c,
            false,
        ),
        Paragraph::default(),
        text("Ваш первый документ", 34, 0x343665, true),
        text(
            "Этот пример можно редактировать. Выделите фразу, чтобы изменить шрифт, цвет или начертание. На вкладке «Вставка» можно добавить дату и специальные символы.",
            24,
            0x353747,
            false,
        ),
        Paragraph::default(),
        text("Ctrl+N     Создать документ", 23, 0x565a75, false),
        text("Ctrl+O     Открыть файл", 23, 0x565a75, false),
        text("Ctrl+S     Сохранить", 23, 0x565a75, false),
    ]
}
