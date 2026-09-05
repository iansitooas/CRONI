use crate::{
    app::UserEvent,
    config::Bookmark,
    downloads::DownloadItem,
    ui::{ChromeState, TOOLBAR_HEIGHT_LOGICAL},
};
use anyhow::{bail, Context, Result};
use std::{ffi::c_void, sync::Mutex};
use windows::{
    core::{w, HSTRING, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
            InvalidateRect, SelectObject, SetBkColor, SetBkMode, SetTextColor, CLEARTYPE_QUALITY,
            CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_END_ELLIPSIS,
            DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL, HBRUSH, HDC,
            HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{EM_SETMARGINS, EM_SETSEL},
            Input::KeyboardAndMouse::{GetFocus, SetFocus, VK_RETURN},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
                DestroyWindow, GetClientRect, GetCursorPos, GetWindowLongPtrW,
                GetWindowTextLengthW, GetWindowTextW, KillTimer, LoadCursorW, RegisterClassW,
                SendMessageW, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
                ShowWindow, TrackPopupMenu, CREATESTRUCTW, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW,
                EC_LEFTMARGIN, EC_RIGHTMARGIN, EN_SETFOCUS, ES_AUTOHSCROLL, GWLP_USERDATA, HMENU,
                IDC_ARROW, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED,
                SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_CREATE,
                WM_CTLCOLOREDIT, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
                WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_SETFOCUS, WM_SETFONT, WM_SIZE, WM_TIMER,
                WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
};
use winit::{
    event_loop::EventLoopProxy,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

const CLASS_NAME: PCWSTR = w!("CRONI_NATIVE_CHROME");
const ADDRESS_ID: usize = 101;
const SPINNER_TIMER: usize = 1;
const MENU_ID_BASE: usize = 1_000;

#[derive(Clone)]
enum Action {
    Command(&'static str),
    SelectTab(u64),
    CloseTab(u64),
    OpenBookmark(String),
    RemoveBookmark(String),
    SetDiscard(u64),
    CancelDownload(u64),
    OpenDownload(u64),
    ShowDownload(u64),
    SettingsMenu,
    DownloadsMenu,
}

#[derive(Clone)]
struct HitRegion {
    rect: RECT,
    action: Action,
}

#[derive(Clone, Default)]
struct OwnedTab {
    id: u64,
    title: String,
    suspended: bool,
}

#[derive(Clone, Default)]
struct OwnedState {
    tabs: Vec<OwnedTab>,
    active_id: u64,
    address: String,
    bookmarked: bool,
    bookmarks: Vec<Bookmark>,
    downloads: Vec<DownloadItem>,
    discard_after_minutes: u64,
    maximized: bool,
    loading: bool,
    blocked_count: u64,
    adblock_enabled: bool,
    adblock_status: String,
    ultra_light_mode: bool,
    reduce_motion: bool,
    pause_media_when_unfocused: bool,
    video_compatibility_mode: bool,
    app_version: String,
    update_configured: bool,
    update_status: String,
    update_version: Option<String>,
    update_ready: bool,
}

impl OwnedState {
    fn from_state(state: &ChromeState<'_>) -> Self {
        Self {
            tabs: state
                .tabs
                .iter()
                .map(|tab| OwnedTab {
                    id: tab.id,
                    title: tab.title.to_string(),
                    suspended: tab.suspended,
                })
                .collect(),
            active_id: state.active_id,
            address: state.address.to_string(),
            bookmarked: state.bookmarked,
            bookmarks: state.bookmarks.to_vec(),
            downloads: state.downloads.to_vec(),
            discard_after_minutes: state.discard_after_minutes,
            maximized: state.maximized,
            loading: state.loading,
            blocked_count: state.blocked_count,
            adblock_enabled: state.adblock_enabled,
            adblock_status: state.adblock_status.to_string(),
            ultra_light_mode: state.ultra_light_mode,
            reduce_motion: state.reduce_motion,
            pause_media_when_unfocused: state.pause_media_when_unfocused,
            video_compatibility_mode: state.video_compatibility_mode,
            app_version: state.app_version.to_string(),
            update_configured: state.update_configured,
            update_status: state.update_status.to_string(),
            update_version: state.update_version.map(str::to_string),
            update_ready: state.update_ready,
        }
    }
}

struct UiData {
    state: OwnedState,
    hits: Vec<HitRegion>,
    scale: f64,
    spinner_phase: bool,
    rendered_address: String,
    select_address_on_release: bool,
}

struct ChromeInner {
    proxy: EventLoopProxy<UserEvent>,
    root: HWND,
    hwnd: HWND,
    edit: HWND,
    edit_brush: HBRUSH,
    font: HFONT,
    ui: Mutex<UiData>,
}

pub struct NativeChrome {
    inner: Box<ChromeInner>,
}

impl NativeChrome {
    pub fn new(window: &Window, proxy: EventLoopProxy<UserEvent>) -> Result<Self> {
        let raw = window
            .window_handle()
            .context("no se pudo obtener la ventana de CRONI")?;
        let RawWindowHandle::Win32(raw) = raw.as_raw() else {
            bail!("CRONI no recibió una ventana Win32");
        };
        let root = HWND(raw.hwnd.get() as *mut c_void);
        let module = unsafe { GetModuleHandleW(None) }?;
        register_class(HINSTANCE(module.0))?;

        let edit_brush = unsafe { CreateSolidBrush(rgb(32, 35, 41)) };
        let font = create_ui_font(window.scale_factor());
        let mut inner = Box::new(ChromeInner {
            proxy,
            root,
            hwnd: HWND::default(),
            edit: HWND::default(),
            edit_brush,
            font,
            ui: Mutex::new(UiData {
                state: OwnedState::default(),
                hits: Vec::new(),
                scale: window.scale_factor(),
                spinner_phase: false,
                rendered_address: String::new(),
                select_address_on_release: false,
            }),
        });
        let pointer = inner.as_mut() as *mut ChromeInner;
        let size = window.inner_size();
        let height = toolbar_height(window.scale_factor());
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS_NAME,
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                0,
                0,
                size.width as i32,
                height,
                Some(root),
                None,
                Some(HINSTANCE(module.0)),
                Some(pointer.cast()),
            )
        }
        .context("no se pudo crear la barra nativa")?;
        inner.hwnd = hwnd;

        let edit = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                0,
                0,
                100,
                32,
                Some(hwnd),
                Some(HMENU(ADDRESS_ID as *mut c_void)),
                Some(HINSTANCE(module.0)),
                None,
            )
        }
        .context("no se pudo crear la barra de direcciones")?;
        inner.edit = edit;
        unsafe {
            SendMessageW(
                edit,
                WM_SETFONT,
                Some(WPARAM(inner.font.0 as usize)),
                Some(LPARAM(1)),
            );
            if !SetWindowSubclass(edit, Some(edit_subclass_proc), 1, pointer as usize).as_bool() {
                bail!("no se pudo conectar la barra de direcciones");
            }
        }

        let chrome = Self { inner };
        chrome.resize(size.width, window.scale_factor());
        Ok(chrome)
    }

    pub fn render(&self, state: &ChromeState<'_>) {
        let owned = OwnedState::from_state(state);
        let (address_changed, loading_changed) = {
            let mut ui = self
                .inner
                .ui
                .lock()
                .unwrap_or_else(|lock| lock.into_inner());
            let address_changed = ui.rendered_address != owned.address;
            let loading_changed = ui.state.loading != owned.loading;
            if address_changed {
                ui.rendered_address = owned.address.clone();
            }
            ui.state = owned;
            (address_changed, loading_changed)
        };
        if address_changed {
            let text = HSTRING::from(
                &self
                    .inner
                    .ui
                    .lock()
                    .unwrap_or_else(|lock| lock.into_inner())
                    .rendered_address,
            );
            let _ = unsafe { SetWindowTextW(self.inner.edit, &text) };
        }
        if loading_changed {
            let loading = self
                .inner
                .ui
                .lock()
                .unwrap_or_else(|lock| lock.into_inner())
                .state
                .loading;
            unsafe {
                if loading {
                    SetTimer(Some(self.inner.hwnd), SPINNER_TIMER, 180, None);
                } else {
                    let _ = KillTimer(Some(self.inner.hwnd), SPINNER_TIMER);
                }
            }
        }
        unsafe {
            let _ = InvalidateRect(Some(self.inner.hwnd), None, false);
        }
    }

    pub fn resize(&self, width: u32, scale: f64) {
        self.inner
            .ui
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .scale = scale;
        unsafe {
            let address_margin = scaled(14, scale).clamp(0, u16::MAX as i32) as u32;
            SendMessageW(
                self.inner.edit,
                EM_SETMARGINS,
                Some(WPARAM((EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize)),
                Some(LPARAM((address_margin | (address_margin << 16)) as isize)),
            );
            let _ = SetWindowPos(
                self.inner.hwnd,
                Some(windows::Win32::UI::WindowsAndMessaging::HWND_TOP),
                0,
                0,
                width as i32,
                toolbar_height(scale),
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(self.inner.hwnd), None, false);
        }
    }

    pub fn set_visible(&self, visible: bool) {
        let loading = self
            .inner
            .ui
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .state
            .loading;
        unsafe {
            if visible && loading {
                SetTimer(Some(self.inner.hwnd), SPINNER_TIMER, 180, None);
            } else {
                let _ = KillTimer(Some(self.inner.hwnd), SPINNER_TIMER);
            }
            let _ = ShowWindow(
                self.inner.hwnd,
                if visible { SW_SHOWNOACTIVATE } else { SW_HIDE },
            );
        }
    }
}

impl Drop for NativeChrome {
    fn drop(&mut self) {
        unsafe {
            if !self.inner.edit.0.is_null() {
                let _ = RemoveWindowSubclass(self.inner.edit, Some(edit_subclass_proc), 1);
            }
            if !self.inner.hwnd.0.is_null() {
                let _ = DestroyWindow(self.inner.hwnd);
            }
            let _ = DeleteObject(HGDIOBJ(self.inner.edit_brush.0));
            let _ = DeleteObject(HGDIOBJ(self.inner.font.0));
        }
    }
}

fn toolbar_height(scale: f64) -> i32 {
    (TOOLBAR_HEIGHT_LOGICAL * scale).round() as i32
}

fn create_ui_font(scale: f64) -> HFONT {
    unsafe {
        CreateFontW(
            -scaled(16, scale),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            w!("Segoe UI"),
        )
    }
}

fn register_class(instance: HINSTANCE) -> Result<()> {
    static CLASS_ATOM: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    let atom = *CLASS_ATOM.get_or_init(|| unsafe {
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(chrome_wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassW(&class)
    });
    if atom == 0 {
        bail!("Windows rechazó la clase de interfaz nativa");
    }
    Ok(())
}

unsafe extern "system" fn chrome_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ChromeInner;
    if pointer.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    let inner = &mut *pointer;
    match message {
        WM_CREATE => LRESULT(0),
        WM_PAINT => {
            paint(inner);
            LRESULT(0)
        }
        WM_SIZE => {
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == SPINNER_TIMER => {
            let mut ui = inner.ui.lock().unwrap_or_else(|lock| lock.into_inner());
            ui.spinner_phase = !ui.spinner_phase;
            drop(ui);
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = point_from_lparam(lparam);
            let action = inner
                .ui
                .lock()
                .unwrap_or_else(|lock| lock.into_inner())
                .hits
                .iter()
                .rev()
                .find(|hit| contains(&hit.rect, x, y))
                .map(|hit| hit.action.clone());
            if let Some(action) = action {
                dispatch_action(inner, action);
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = point_from_lparam(lparam);
            let ui = inner.ui.lock().unwrap_or_else(|lock| lock.into_inner());
            let is_drag_area = ui
                .hits
                .iter()
                .rev()
                .find(|hit| contains(&hit.rect, x, y))
                .is_some_and(|hit| matches!(hit.action, Action::Command("window_drag")));
            drop(ui);
            if is_drag_area {
                send_json(
                    inner,
                    serde_json::json!({ "type": "window_toggle_maximize" }),
                );
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let control_id = wparam.0 & 0xffff;
            let notification = (wparam.0 >> 16) & 0xffff;
            if control_id == ADDRESS_ID && notification == EN_SETFOCUS as usize {
                SendMessageW(inner.edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as *mut c_void);
            SetTextColor(hdc, rgb(247, 248, 250));
            SetBkColor(hdc, rgb(32, 35, 41));
            LRESULT(inner.edit_brush.0 as isize)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn edit_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    reference: usize,
) -> LRESULT {
    let inner = &mut *(reference as *mut ChromeInner);
    match message {
        WM_LBUTTONDOWN => {
            if GetFocus() != hwnd {
                inner
                    .ui
                    .lock()
                    .unwrap_or_else(|lock| lock.into_inner())
                    .select_address_on_release = true;
            }
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
        WM_LBUTTONUP => {
            let result = DefSubclassProc(hwnd, message, wparam, lparam);
            let mut ui = inner.ui.lock().unwrap_or_else(|lock| lock.into_inner());
            if ui.select_address_on_release {
                ui.select_address_on_release = false;
                drop(ui);
                SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            }
            result
        }
        WM_SETFOCUS => {
            let result = DefSubclassProc(hwnd, message, wparam, lparam);
            SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            result
        }
        WM_KEYDOWN if wparam.0 == VK_RETURN.0 as usize => {
            let length = GetWindowTextLengthW(hwnd).max(0) as usize;
            let mut buffer = vec![0u16; length + 1];
            let read = GetWindowTextW(hwnd, &mut buffer).max(0) as usize;
            let address = String::from_utf16_lossy(&buffer[..read]);
            send_json(
                inner,
                serde_json::json!({ "type": "navigate", "url": address }),
            );
            let _ = SetFocus(Some(inner.root));
            LRESULT(0)
        }
        _ => DefSubclassProc(hwnd, message, wparam, lparam),
    }
}

unsafe fn paint(inner: &ChromeInner) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(inner.hwnd, &mut paint);
    let mut client = RECT::default();
    let _ = GetClientRect(inner.hwnd, &mut client);
    let (state, scale, spinner_phase) = {
        let ui = inner.ui.lock().unwrap_or_else(|lock| lock.into_inner());
        (ui.state.clone(), ui.scale, ui.spinner_phase)
    };
    SelectObject(hdc, HGDIOBJ(inner.font.0));
    SetBkMode(hdc, TRANSPARENT);

    fill(
        hdc,
        rect(0, 0, client.right, scaled(45, scale)),
        rgb(17, 19, 24),
    );
    fill(
        hdc,
        rect(0, scaled(45, scale), client.right, scaled(108, scale)),
        rgb(48, 52, 61),
    );
    fill(
        hdc,
        rect(0, scaled(108, scale), client.right, client.bottom),
        rgb(36, 39, 46),
    );

    let mut hits = Vec::new();
    paint_tabs(hdc, &state, scale, client.right, &mut hits);
    paint_navigation(
        inner,
        hdc,
        &state,
        scale,
        client.right,
        spinner_phase,
        &mut hits,
    );
    paint_shortcuts(hdc, &state, scale, client.right, &mut hits);
    inner
        .ui
        .lock()
        .unwrap_or_else(|lock| lock.into_inner())
        .hits = hits;
    let _ = EndPaint(inner.hwnd, &paint);
}

unsafe fn paint_tabs(
    hdc: HDC,
    state: &OwnedState,
    scale: f64,
    width: i32,
    hits: &mut Vec<HitRegion>,
) {
    let row = rect(0, 0, width, scaled(45, scale));
    hits.push(HitRegion {
        rect: row,
        action: Action::Command("window_drag"),
    });
    let control_width = scaled(46, scale);
    let controls_left = width - control_width * 3;
    let x_close = controls_left + control_width * 2;
    draw_button(
        hdc,
        rect(x_close, 0, width, scaled(45, scale)),
        "✕",
        false,
        rgb(242, 243, 246),
        Action::Command("window_close"),
        hits,
    );
    draw_button(
        hdc,
        rect(controls_left + control_width, 0, x_close, scaled(45, scale)),
        if state.maximized { "❐" } else { "□" },
        false,
        rgb(216, 219, 226),
        Action::Command("window_toggle_maximize"),
        hits,
    );
    draw_button(
        hdc,
        rect(
            controls_left,
            0,
            controls_left + control_width,
            scaled(45, scale),
        ),
        "—",
        false,
        rgb(216, 219, 226),
        Action::Command("window_minimize"),
        hits,
    );

    let left = scaled(8, scale);
    let plus_width = scaled(34, scale);
    let available = (controls_left - left - plus_width - scaled(8, scale)).max(scaled(90, scale));
    let min_tab = scaled(92, scale).max(1);
    let visible_count = ((available / min_tab).max(1) as usize).min(state.tabs.len().max(1));
    let active_index = state
        .tabs
        .iter()
        .position(|tab| tab.id == state.active_id)
        .unwrap_or(0);
    let start = active_index
        .saturating_sub(visible_count / 2)
        .min(state.tabs.len().saturating_sub(visible_count));
    let end = (start + visible_count).min(state.tabs.len());
    let count = end.saturating_sub(start).max(1) as i32;
    let tab_width = (available / count).min(scaled(210, scale)).max(min_tab);
    let mut x = left;
    for tab in &state.tabs[start..end] {
        let tab_rect = rect(
            x,
            scaled(5, scale),
            x + tab_width - scaled(4, scale),
            scaled(39, scale),
        );
        let active = tab.id == state.active_id;
        if active {
            fill(hdc, tab_rect, rgb(48, 52, 61));
        } else {
            fill(hdc, tab_rect, rgb(36, 39, 46));
        }
        hits.push(HitRegion {
            rect: tab_rect,
            action: Action::SelectTab(tab.id),
        });
        let close_rect = rect(
            tab_rect.right - scaled(27, scale),
            tab_rect.top,
            tab_rect.right,
            tab_rect.bottom,
        );
        let label_rect = rect(
            tab_rect.left + scaled(9, scale),
            tab_rect.top,
            close_rect.left - scaled(2, scale),
            tab_rect.bottom,
        );
        let label = if tab.suspended {
            format!("○ {}", tab.title)
        } else {
            tab.title.clone()
        };
        draw_label(
            hdc,
            label_rect,
            &label,
            if active {
                rgb(255, 255, 255)
            } else {
                rgb(184, 189, 201)
            },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        draw_label(
            hdc,
            close_rect,
            "×",
            rgb(220, 223, 230),
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        hits.push(HitRegion {
            rect: close_rect,
            action: Action::CloseTab(tab.id),
        });
        x += tab_width;
    }
    draw_button(
        hdc,
        rect(x, scaled(5, scale), x + plus_width, scaled(39, scale)),
        "+",
        false,
        rgb(231, 233, 238),
        Action::Command("new_tab"),
        hits,
    );
}

unsafe fn paint_navigation(
    inner: &ChromeInner,
    hdc: HDC,
    state: &OwnedState,
    scale: f64,
    width: i32,
    spinner_phase: bool,
    hits: &mut Vec<HitRegion>,
) {
    let top = scaled(45, scale);
    let bottom = scaled(108, scale);
    let button = scaled(34, scale);
    let gap = scaled(5, scale);
    let y = top + scaled(12, scale);
    let mut x = scaled(10, scale);
    for (label, command) in [
        ("←", "back"),
        ("→", "forward"),
        (
            if state.loading && spinner_phase {
                "⟳"
            } else {
                "↻"
            },
            "reload",
        ),
        ("⌂", "home"),
    ] {
        draw_button(
            hdc,
            rect(x, y, x + button, y + button),
            label,
            false,
            rgb(231, 233, 238),
            Action::Command(command),
            hits,
        );
        x += button + gap;
    }

    let mut right = width - scaled(10, scale);
    let menu_rect = take_right(&mut right, button, gap, y, button);
    draw_button(
        hdc,
        menu_rect,
        "☰",
        false,
        rgb(231, 233, 238),
        Action::SettingsMenu,
        hits,
    );
    let downloads_rect = take_right(&mut right, button, gap, y, button);
    draw_button(
        hdc,
        downloads_rect,
        "⇩",
        state
            .downloads
            .iter()
            .any(|item| matches!(item.status.as_str(), "downloading" | "cancelling")),
        rgb(231, 233, 238),
        Action::DownloadsMenu,
        hits,
    );
    let bookmark_rect = take_right(&mut right, button, gap, y, button);
    draw_button(
        hdc,
        bookmark_rect,
        if state.bookmarked { "★" } else { "☆" },
        state.bookmarked,
        if state.bookmarked {
            rgb(255, 207, 85)
        } else {
            rgb(231, 233, 238)
        },
        Action::Command("toggle_bookmark"),
        hits,
    );
    if state.update_ready {
        let update_rect = take_right(&mut right, button, gap, y, button);
        draw_button(
            hdc,
            update_rect,
            "⇧",
            true,
            rgb(143, 227, 159),
            Action::Command("install_update"),
            hits,
        );
    }
    let shield_width = scaled(58, scale);
    let shield_rect = take_right(&mut right, shield_width, gap, y, button);
    draw_button(
        hdc,
        shield_rect,
        &format!("◇ {}", state.blocked_count.min(999)),
        state.adblock_enabled,
        if state.adblock_enabled {
            rgb(112, 167, 255)
        } else {
            rgb(150, 154, 164)
        },
        Action::Command("toggle_adblock"),
        hits,
    );
    let edit_left = x + scaled(5, scale);
    let edit_right = (right - scaled(5, scale)).max(edit_left + scaled(100, scale));
    let vertical_text_offset = scaled(5, scale);
    let edit_top = y + vertical_text_offset;
    let edit_height = (button - vertical_text_offset).max(scaled(24, scale));
    fill(
        hdc,
        rect(edit_left, y, edit_right, y + button),
        rgb(32, 35, 41),
    );
    let _ = SetWindowPos(
        inner.edit,
        None,
        edit_left,
        edit_top,
        (edit_right - edit_left).max(20),
        edit_height,
        SWP_NOACTIVATE,
    );
    let border = rect(edit_left - 1, y - 1, edit_right + 1, y + button + 1);
    frame(hdc, border, rgb(68, 73, 86));
    let _ = bottom;
}

unsafe fn paint_shortcuts(
    hdc: HDC,
    state: &OwnedState,
    scale: f64,
    width: i32,
    hits: &mut Vec<HitRegion>,
) {
    let top = scaled(108, scale);
    let bottom = scaled(144, scale);
    let mut x = scaled(10, scale);
    for bookmark in &state.bookmarks {
        let item_width = scaled(
            (bookmark.title.chars().count() as i32 * 7 + 42).clamp(88, 190),
            scale,
        );
        if x + item_width + scaled(130, scale) > width {
            break;
        }
        let item = rect(
            x,
            top + scaled(4, scale),
            x + item_width,
            bottom - scaled(5, scale),
        );
        let close = rect(
            item.right - scaled(24, scale),
            item.top,
            item.right,
            item.bottom,
        );
        draw_label(
            hdc,
            rect(
                item.left + scaled(7, scale),
                item.top,
                close.left,
                item.bottom,
            ),
            &bookmark.title,
            rgb(233, 235, 240),
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        draw_label(
            hdc,
            close,
            "×",
            rgb(159, 166, 180),
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        hits.push(HitRegion {
            rect: item,
            action: Action::OpenBookmark(bookmark.url.clone()),
        });
        hits.push(HitRegion {
            rect: close,
            action: Action::RemoveBookmark(bookmark.url.clone()),
        });
        x += item_width + scaled(4, scale);
    }
    let add = rect(
        x,
        top + scaled(4, scale),
        (x + scaled(122, scale)).min(width),
        bottom - scaled(5, scale),
    );
    draw_button(
        hdc,
        add,
        "+ Añadir acceso",
        false,
        rgb(174, 181, 194),
        Action::Command("add_shortcut"),
        hits,
    );
}

unsafe fn draw_button(
    hdc: HDC,
    bounds: RECT,
    label: &str,
    active: bool,
    color: COLORREF,
    action: Action,
    hits: &mut Vec<HitRegion>,
) {
    if active {
        fill(hdc, bounds, rgb(58, 65, 78));
    }
    draw_label(
        hdc,
        bounds,
        label,
        color,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
    hits.push(HitRegion {
        rect: bounds,
        action,
    });
}

unsafe fn draw_label(
    hdc: HDC,
    mut bounds: RECT,
    label: &str,
    color: COLORREF,
    flags: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT,
) {
    let mut text = label.encode_utf16().collect::<Vec<_>>();
    SetTextColor(hdc, color);
    DrawTextW(hdc, &mut text, &mut bounds, flags);
}

unsafe fn fill(hdc: HDC, bounds: RECT, color: COLORREF) {
    let brush = CreateSolidBrush(color);
    FillRect(hdc, &bounds, brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

unsafe fn frame(hdc: HDC, bounds: RECT, color: COLORREF) {
    fill(
        hdc,
        rect(bounds.left, bounds.top, bounds.right, bounds.top + 1),
        color,
    );
    fill(
        hdc,
        rect(bounds.left, bounds.bottom - 1, bounds.right, bounds.bottom),
        color,
    );
    fill(
        hdc,
        rect(bounds.left, bounds.top, bounds.left + 1, bounds.bottom),
        color,
    );
    fill(
        hdc,
        rect(bounds.right - 1, bounds.top, bounds.right, bounds.bottom),
        color,
    );
}

unsafe fn dispatch_action(inner: &ChromeInner, action: Action) {
    match action {
        Action::SettingsMenu => show_settings_menu(inner),
        Action::DownloadsMenu => show_downloads_menu(inner),
        Action::Command(command) => send_json(inner, serde_json::json!({ "type": command })),
        Action::SelectTab(id) => {
            send_json(inner, serde_json::json!({ "type": "select_tab", "id": id }))
        }
        Action::CloseTab(id) => {
            send_json(inner, serde_json::json!({ "type": "close_tab", "id": id }))
        }
        Action::OpenBookmark(url) => send_json(
            inner,
            serde_json::json!({ "type": "open_bookmark", "url": url }),
        ),
        Action::RemoveBookmark(url) => send_json(
            inner,
            serde_json::json!({ "type": "remove_bookmark", "url": url }),
        ),
        Action::SetDiscard(minutes) => send_json(
            inner,
            serde_json::json!({ "type": "set_discard", "minutes": minutes }),
        ),
        Action::CancelDownload(id) => send_json(
            inner,
            serde_json::json!({ "type": "cancel_download", "id": id }),
        ),
        Action::OpenDownload(id) => send_json(
            inner,
            serde_json::json!({ "type": "open_download", "id": id }),
        ),
        Action::ShowDownload(id) => send_json(
            inner,
            serde_json::json!({ "type": "show_download", "id": id }),
        ),
    }
}

unsafe fn show_settings_menu(inner: &ChromeInner) {
    let state = inner
        .ui
        .lock()
        .unwrap_or_else(|lock| lock.into_inner())
        .state
        .clone();
    let Ok(menu) = CreatePopupMenu() else { return };
    let mut actions = Vec::new();
    append_action(
        menu,
        "Establecer CRONI como predeterminado",
        MF_STRING,
        Action::Command("set_default_browser"),
        &mut actions,
    );
    append_separator(menu);
    if state.update_ready {
        append_action(
            menu,
            &format!(
                "Actualizar a {}",
                state.update_version.as_deref().unwrap_or("nueva versión")
            ),
            MF_STRING,
            Action::Command("install_update"),
            &mut actions,
        );
    } else if matches!(state.update_status.as_str(), "current" | "failed") {
        append_action(
            menu,
            "Buscar actualizaciones",
            MF_STRING,
            Action::Command("check_update"),
            &mut actions,
        );
    } else {
        let label = if state.update_configured {
            format!("CRONI {} · buscando actualización", state.app_version)
        } else {
            format!("CRONI {}", state.app_version)
        };
        append_disabled(menu, &label);
    }
    append_separator(menu);
    append_action(
        menu,
        "Modo ultraligero",
        MF_STRING
            | if state.ultra_light_mode {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            },
        Action::Command("toggle_ultra_light"),
        &mut actions,
    );
    append_action(
        menu,
        "Reducir animaciones web",
        MF_STRING
            | if state.reduce_motion {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            },
        Action::Command("toggle_reduce_motion"),
        &mut actions,
    );
    append_action(
        menu,
        "Pausar multimedia al salir",
        MF_STRING
            | if state.pause_media_when_unfocused {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            },
        Action::Command("toggle_background_pause"),
        &mut actions,
    );
    append_action(
        menu,
        "Video compatible sin GPU (reiniciar CRONI)",
        MF_STRING
            | if state.video_compatibility_mode {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            },
        Action::Command("toggle_video_compatibility"),
        &mut actions,
    );
    let filter_status = match state.adblock_status.as_str() {
        "ready" => "EasyList y reglas Brave listas",
        "updating" => "actualizando listas",
        "failed" => "protección básica; falló la actualización",
        _ => "protección básica",
    };
    append_disabled(menu, &format!("Bloqueador: {filter_status}"));
    if !state.ultra_light_mode {
        let Ok(discard_menu) = CreatePopupMenu() else {
            let _ = DestroyMenu(menu);
            return;
        };
        for minutes in [1, 5, 15, 30, 60] {
            append_action(
                discard_menu,
                &format!("{minutes} minuto{}", if minutes == 1 { "" } else { "s" }),
                MF_STRING
                    | if state.discard_after_minutes == minutes {
                        MF_CHECKED
                    } else {
                        MF_UNCHECKED
                    },
                Action::SetDiscard(minutes),
                &mut actions,
            );
        }
        append_submenu(menu, discard_menu, "Descartar pestañas tras");
    }
    if !state.bookmarks.is_empty() {
        append_separator(menu);
        let Ok(bookmarks_menu) = CreatePopupMenu() else {
            let _ = DestroyMenu(menu);
            return;
        };
        for bookmark in state.bookmarks.iter().take(30) {
            append_action(
                bookmarks_menu,
                &bookmark.title,
                MF_STRING,
                Action::OpenBookmark(bookmark.url.clone()),
                &mut actions,
            );
        }
        append_submenu(menu, bookmarks_menu, "Marcadores");
    }
    show_menu(inner, menu, actions);
}

unsafe fn show_downloads_menu(inner: &ChromeInner) {
    let downloads = inner
        .ui
        .lock()
        .unwrap_or_else(|lock| lock.into_inner())
        .state
        .downloads
        .clone();
    let Ok(menu) = CreatePopupMenu() else { return };
    let mut actions = Vec::new();
    if downloads.is_empty() {
        append_disabled(menu, "Todavía no hay descargas");
    } else {
        for item in downloads.iter().rev().take(20) {
            let Ok(item_menu) = CreatePopupMenu() else {
                continue;
            };
            match item.status.as_str() {
                "downloading" | "cancelling" => append_action(
                    item_menu,
                    "Cancelar descarga",
                    if item.status == "cancelling" {
                        MF_STRING | MF_GRAYED
                    } else {
                        MF_STRING
                    },
                    Action::CancelDownload(item.id),
                    &mut actions,
                ),
                "completed" => {
                    append_action(
                        item_menu,
                        "Abrir archivo",
                        MF_STRING,
                        Action::OpenDownload(item.id),
                        &mut actions,
                    );
                    append_action(
                        item_menu,
                        "Mostrar en carpeta",
                        MF_STRING,
                        Action::ShowDownload(item.id),
                        &mut actions,
                    );
                }
                _ => append_action(
                    item_menu,
                    "Mostrar ubicación",
                    MF_STRING,
                    Action::ShowDownload(item.id),
                    &mut actions,
                ),
            }
            let progress = item
                .total
                .filter(|total| *total > 0)
                .map(|total| {
                    format!(
                        " · {}%",
                        item.received
                            .saturating_mul(100)
                            .saturating_div(total)
                            .min(100)
                    )
                })
                .unwrap_or_default();
            append_submenu(menu, item_menu, &format!("{}{}", item.name, progress));
        }
        append_separator(menu);
        append_action(
            menu,
            "Limpiar historial terminado",
            MF_STRING,
            Action::Command("clear_downloads"),
            &mut actions,
        );
    }
    show_menu(inner, menu, actions);
}

unsafe fn show_menu(inner: &ChromeInner, menu: HMENU, actions: Vec<Action>) {
    let mut cursor = POINT::default();
    if GetCursorPos(&mut cursor).is_ok() {
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            None,
            inner.hwnd,
            None,
        )
        .0 as usize;
        if let Some(action) = chosen
            .checked_sub(MENU_ID_BASE)
            .and_then(|index| actions.get(index))
            .cloned()
        {
            dispatch_action(inner, action);
        }
    }
    let _ = DestroyMenu(menu);
}

unsafe fn append_action(
    menu: HMENU,
    label: &str,
    flags: windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS,
    action: Action,
    actions: &mut Vec<Action>,
) {
    let id = MENU_ID_BASE + actions.len();
    actions.push(action);
    let label = HSTRING::from(menu_text(label));
    let _ = AppendMenuW(menu, flags, id, &label);
}

unsafe fn append_disabled(menu: HMENU, label: &str) {
    let label = HSTRING::from(menu_text(label));
    let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, &label);
}

unsafe fn append_separator(menu: HMENU) {
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
}

unsafe fn append_submenu(menu: HMENU, submenu: HMENU, label: &str) {
    let label = HSTRING::from(menu_text(label));
    let _ = AppendMenuW(menu, MF_POPUP | MF_STRING, submenu.0 as usize, &label);
}

fn menu_text(text: &str) -> String {
    text.replace('&', "&&")
}

fn send_json(inner: &ChromeInner, value: serde_json::Value) {
    let _ = inner
        .proxy
        .send_event(UserEvent::ChromeCommand(value.to_string()));
}

fn take_right(right: &mut i32, width: i32, gap: i32, y: i32, height: i32) -> RECT {
    let bounds = rect(*right - width, y, *right, y + height);
    *right -= width + gap;
    bounds
}

fn point_from_lparam(value: LPARAM) -> (i32, i32) {
    let packed = value.0 as u32;
    (
        (packed as u16) as i16 as i32,
        ((packed >> 16) as u16) as i16 as i32,
    )
}

fn contains(bounds: &RECT, x: i32, y: i32) -> bool {
    x >= bounds.left && x < bounds.right && y >= bounds.top && y < bounds.bottom
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

fn scaled(value: i32, scale: f64) -> i32 {
    (value as f64 * scale).round() as i32
}

fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(red as u32 | ((green as u32) << 8) | ((blue as u32) << 16))
}
