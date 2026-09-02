#[cfg(target_os = "windows")]
use crate::native_chrome::NativeChrome;
#[cfg(not(target_os = "windows"))]
use crate::ui::CHROME_HTML;
use crate::{
    blocker,
    config::{app_data_dir, AppConfig, Bookmark},
    default_browser,
    downloads::{self, DownloadItem},
    navigation::normalize_address,
    ui::{ChromeState, TabState, TOOLBAR_HEIGHT_LOGICAL},
    updater,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy},
    window::{Fullscreen, Icon, Window, WindowId},
};
use wry::{
    dpi::{PhysicalPosition, PhysicalSize},
    NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder,
};

#[cfg(target_os = "windows")]
use wry::{MemoryUsageLevel, WebViewBuilderExtWindows, WebViewExtWindows};

#[cfg(target_os = "windows")]
use winit::platform::windows::WindowAttributesExtWindows;

#[cfg(target_os = "windows")]
use windows::{
    core::HSTRING,
    Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems},
};

#[cfg(target_os = "windows")]
const FULLSCREEN_VIDEO_FIX_SCRIPT: &str = include_str!("../assets/fullscreen_video_fix.js");

#[derive(Debug)]
pub enum UserEvent {
    ChromeCommand(String),
    PageLoadChanged {
        tab_id: u64,
        url: String,
        loading: bool,
    },
    LocationChanged {
        tab_id: u64,
        url: String,
    },
    TitleChanged {
        tab_id: u64,
        title: String,
    },
    OpenInNewTab(String),
    DownloadStarted {
        id: u64,
        name: String,
        url: String,
        path: String,
        received: u64,
        total: Option<u64>,
    },
    DownloadProgress {
        id: u64,
        received: u64,
        total: Option<u64>,
        status: String,
    },
    UpdateReady {
        version: String,
        path: PathBuf,
        sha256: String,
    },
    UpdateCurrent,
    UpdateFailed(String),
    ContentFullscreenChanged {
        tab_id: u64,
        fullscreen: bool,
    },
}

struct Tab {
    id: u64,
    url: String,
    title: String,
    last_active: Instant,
    loading: bool,
    view: Option<WebView>,
}

pub struct BrowserApp {
    proxy: EventLoopProxy<UserEvent>,
    config: AppConfig,
    context: WebContext,
    window: Option<Window>,
    #[cfg(target_os = "windows")]
    chrome: Option<NativeChrome>,
    #[cfg(not(target_os = "windows"))]
    chrome: Option<WebView>,
    tabs: Vec<Tab>,
    active: usize,
    next_tab_id: u64,
    window_is_focused: bool,
    content_fullscreen: bool,
    refresh_surface_after_load: Option<u64>,
    downloads_panel_open: bool,
    downloads: Vec<DownloadItem>,
    blocker: blocker::BlockerManager,
    update_status: String,
    update_version: Option<String>,
    update_path: Option<PathBuf>,
    update_sha256: Option<String>,
    #[cfg(target_os = "windows")]
    download_operations: downloads::OperationMap,
    #[cfg(target_os = "windows")]
    next_download_id: downloads::DownloadIdCounter,
}

impl BrowserApp {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Result<Self> {
        let config = AppConfig::load();
        let blocker = blocker::BlockerManager::new(&config.adblock_disabled_hosts);
        let external_url = std::env::args()
            .nth(1)
            .filter(|url| blocker::is_navigation_allowed(url));
        let data_dir = app_data_dir().join("WebView2Data");
        std::fs::create_dir_all(&data_dir).context("no se pudo crear el directorio de datos")?;

        let urls = if let Some(url) = external_url {
            vec![url]
        } else if config.restore_urls.is_empty() {
            vec![config.home_url.clone()]
        } else {
            config.restore_urls.clone()
        };
        let now = Instant::now();
        let tabs = urls
            .into_iter()
            .enumerate()
            .map(|(index, url)| Tab {
                id: index as u64 + 1,
                title: title_from_url(&url),
                url,
                last_active: now,
                loading: false,
                view: None,
            })
            .collect::<Vec<_>>();
        let next_tab_id = tabs.len() as u64 + 1;
        let downloads = config
            .downloads
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, item)| DownloadItem::from_stored(index as u64 + 1, item))
            .collect::<Vec<_>>();
        let next_download_id = downloads.len() as u64 + 1;

        Ok(Self {
            proxy,
            config,
            context: WebContext::new(Some(data_dir)),
            window: None,
            chrome: None,
            tabs,
            active: 0,
            next_tab_id,
            window_is_focused: true,
            content_fullscreen: false,
            refresh_surface_after_load: None,
            downloads_panel_open: false,
            downloads,
            blocker,
            update_status: if updater::is_configured() {
                "checking".into()
            } else {
                "disabled".into()
            },
            update_version: None,
            update_path: None,
            update_sha256: None,
            #[cfg(target_os = "windows")]
            download_operations: downloads::new_operation_map(),
            #[cfg(target_os = "windows")]
            next_download_id: downloads::new_id_counter(next_download_id),
        })
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let icon = app_icon();
        let attributes = Window::default_attributes()
            .with_title("CRONI")
            .with_inner_size(LogicalSize::new(1120.0, 760.0))
            .with_min_inner_size(LogicalSize::new(680.0, 420.0))
            .with_decorations(false)
            .with_window_icon(icon.clone());
        #[cfg(target_os = "windows")]
        let attributes = attributes
            .with_taskbar_icon(icon)
            .with_undecorated_shadow(true);
        let window = event_loop.create_window(attributes)?;

        #[cfg(target_os = "windows")]
        let chrome = NativeChrome::new(&window, self.proxy.clone())
            .context("no se pudo crear la interfaz nativa")?;
        #[cfg(not(target_os = "windows"))]
        let chrome = WebViewBuilder::new()
            .with_html(CHROME_HTML)
            .with_bounds(toolbar_bounds(&window, self.downloads_panel_open))
            .with_transparent(true)
            .with_incognito(true)
            .with_devtools(cfg!(debug_assertions))
            .with_ipc_handler({
                let proxy = self.proxy.clone();
                move |request| {
                    let _ = proxy.send_event(UserEvent::ChromeCommand(request.body().clone()));
                }
            })
            .build_as_child(&window)
            .context("no se pudo crear la interfaz del navegador")?;

        self.window = Some(window);
        self.chrome = Some(chrome);
        self.ensure_active_view()?;
        self.resize_views();
        self.render_chrome();
        self.blocker.start_filter_update();
        updater::start_update_check(self.proxy.clone());
        Ok(())
    }

    fn ensure_active_view(&mut self) -> Result<()> {
        if self.tabs[self.active].view.is_some() {
            return Ok(());
        }

        let window = self.window.as_ref().context("ventana aún no creada")?;
        let tab_id = self.tabs[self.active].id;
        let url = self.tabs[self.active].url.clone();
        let bounds = content_bounds(window, self.content_fullscreen);
        let location_proxy = self.proxy.clone();
        let title_proxy = self.proxy.clone();
        let popup_proxy = self.proxy.clone();
        let page_url = Arc::new(RwLock::new(url.clone()));
        let page_url_for_events = page_url.clone();

        let builder = WebViewBuilder::new_with_web_context(&mut self.context)
            .with_url(&url)
            .with_bounds(bounds)
            .with_visible(true)
            .with_focused(true)
            .with_autoplay(false)
            .with_clipboard(true)
            .with_general_autofill_enabled(false)
            .with_devtools(cfg!(debug_assertions))
            .with_initialization_script(blocker::INITIALIZATION_SCRIPT)
            .with_navigation_handler(|target| {
                blocker::is_navigation_allowed(&target) && !blocker::is_blocked_url(&target)
            })
            .with_on_page_load_handler(move |event, url| {
                *page_url_for_events
                    .write()
                    .unwrap_or_else(|lock| lock.into_inner()) = url.clone();
                let _ = location_proxy.send_event(UserEvent::PageLoadChanged {
                    tab_id,
                    url,
                    loading: matches!(event, PageLoadEvent::Started),
                });
            })
            .with_document_title_changed_handler(move |title| {
                let _ = title_proxy.send_event(UserEvent::TitleChanged { tab_id, title });
            })
            .with_new_window_req_handler(move |url, _features| {
                let _ = popup_proxy.send_event(UserEvent::OpenInNewTab(url));
                NewWindowResponse::Deny
            });

        let builder = if self.config.reduce_motion {
            builder.with_initialization_script(blocker::PERFORMANCE_SCRIPT)
        } else {
            builder
        };

        #[cfg(target_os = "windows")]
        let builder = builder
            .with_initialization_script(FULLSCREEN_VIDEO_FIX_SCRIPT)
            .with_browser_extensions_enabled(false)
            .with_browser_accelerator_keys(false);

        let view = builder
            .build_as_child(window)
            .with_context(|| format!("no se pudo abrir {url}"))?;
        blocker::attach_native_protections(&view, self.blocker.clone(), page_url.clone())
            .context("no se pudieron activar las protecciones de red")?;
        #[cfg(target_os = "windows")]
        attach_location_changed_handler(&view, self.proxy.clone(), tab_id, page_url.clone())?;
        #[cfg(target_os = "windows")]
        downloads::attach_download_manager(
            &view,
            self.proxy.clone(),
            self.next_download_id.clone(),
            self.download_operations.clone(),
        )?;
        #[cfg(target_os = "windows")]
        attach_fullscreen_handler(&view, self.proxy.clone(), tab_id)?;
        set_memory_level(&view, self.window_is_focused);
        self.tabs[self.active].loading = true;
        self.tabs[self.active].view = Some(view);
        Ok(())
    }

    fn add_tab(&mut self, raw_url: &str, activate: bool) {
        let url = normalize_address(raw_url, &self.config.search_url);
        let tab = Tab {
            id: self.next_tab_id,
            title: title_from_url(&url),
            url,
            last_active: Instant::now(),
            loading: false,
            view: None,
        };
        self.next_tab_id += 1;
        self.tabs.push(tab);
        if activate {
            let target = self.tabs.len() - 1;
            self.switch_to(target);
        }
        self.persist_session();
        self.render_chrome();
    }

    fn switch_to(&mut self, target: usize) {
        if target >= self.tabs.len() {
            return;
        }
        let now = Instant::now();
        if target != self.active {
            if self.content_fullscreen {
                let current_id = self.tabs[self.active].id;
                if let Some(view) = self.tabs[self.active].view.as_ref() {
                    let _ = view.evaluate_script(
                        "if (document.fullscreenElement) document.exitFullscreen();",
                    );
                }
                self.set_content_fullscreen(current_id, false);
            }
            if let Some(view) = self.tabs[self.active].view.as_ref() {
                let _ = view.evaluate_script(blocker::PAUSE_MEDIA_SCRIPT);
                let _ = view.set_visible(false);
                set_memory_level(view, false);
            }
            self.tabs[self.active].last_active = now;
            if self.config.ultra_light_mode {
                self.tabs[self.active].view = None;
            }
            self.active = target;
        }
        self.tabs[self.active].last_active = now;

        if let Err(error) = self.ensure_active_view() {
            eprintln!("Error al restaurar pestaña: {error:#}");
        }
        if let Some(view) = self.tabs[self.active].view.as_ref() {
            let _ = view.set_visible(true);
            let _ = view.focus();
            set_memory_level(view, self.window_is_focused);
        }
        self.render_chrome();
    }

    fn close_tab_by_id(&mut self, id: u64) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let was_active = index == self.active;
        self.tabs.remove(index);

        if self.tabs.is_empty() {
            self.active = 0;
            self.add_tab(&self.config.home_url.clone(), true);
            return;
        }
        if index < self.active || self.active == self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        if was_active {
            self.switch_to(self.active);
        }
        self.persist_session();
        self.render_chrome();
    }

    fn navigate_active(&mut self, raw: &str) {
        let url = normalize_address(raw, &self.config.search_url);
        self.tabs[self.active].url = url.clone();
        if self.tabs[self.active].view.is_none() {
            if let Err(error) = self.ensure_active_view() {
                eprintln!("Error al abrir dirección: {error:#}");
            }
        } else if let Some(view) = self.tabs[self.active].view.as_ref() {
            let _ = view.evaluate_script(blocker::PAUSE_MEDIA_SCRIPT);
            if let Err(error) = view.load_url(&url) {
                eprintln!("Error de navegación: {error}");
            }
        }
        self.persist_session();
        self.render_chrome();
    }

    fn reload_active(&mut self) {
        let tab_id = self.tabs[self.active].id;
        if self.content_fullscreen {
            if let Some(view) = self.tabs[self.active].view.as_ref() {
                let _ = view
                    .evaluate_script("if (document.fullscreenElement) document.exitFullscreen();");
            }
            self.set_content_fullscreen(tab_id, false);
        }
        self.refresh_surface_after_load = Some(tab_id);
        if let Some(view) = self.tabs[self.active].view.as_ref() {
            if let Err(error) = view.reload() {
                self.refresh_surface_after_load = None;
                eprintln!("Error al recargar: {error}");
            }
        }
    }

    fn handle_chrome_command(&mut self, raw: &str, event_loop: &ActiveEventLoop) {
        let Ok(command) = serde_json::from_str::<Value>(raw) else {
            return;
        };
        match command
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "navigate" => {
                if let Some(url) = command.get("url").and_then(Value::as_str) {
                    self.navigate_active(url);
                }
            }
            "back" => self.navigate_with(|view| view.go_back()),
            "forward" => self.navigate_with(|view| view.go_forward()),
            "reload" => self.reload_active(),
            "home" => self.navigate_active(&self.config.home_url.clone()),
            "new_tab" => self.add_tab(&self.config.home_url.clone(), true),
            "select_tab" => {
                if let Some(id) = command.get("id").and_then(Value::as_u64) {
                    if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                        self.switch_to(index);
                    }
                }
            }
            "close_tab" => {
                if let Some(id) = command.get("id").and_then(Value::as_u64) {
                    self.close_tab_by_id(id);
                }
            }
            "toggle_bookmark" => self.toggle_bookmark(),
            "add_shortcut" => self.add_active_bookmark(),
            "remove_bookmark" => {
                if let Some(url) = command.get("url").and_then(Value::as_str) {
                    self.config.bookmarks.retain(|item| item.url != url);
                    self.persist_session();
                    self.render_chrome();
                }
            }
            "open_bookmark" => {
                if let Some(url) = command.get("url").and_then(Value::as_str) {
                    self.navigate_active(url);
                }
            }
            "set_discard" => {
                if let Some(minutes) = command.get("minutes").and_then(Value::as_u64) {
                    self.config.discard_after_minutes = minutes.clamp(1, 120);
                    self.persist_session();
                    self.render_chrome();
                }
            }
            "toggle_ultra_light" => {
                self.config.ultra_light_mode = !self.config.ultra_light_mode;
                if self.config.ultra_light_mode {
                    self.discard_expired_tabs(true);
                }
                self.persist_session();
                self.render_chrome();
            }
            "toggle_reduce_motion" => {
                self.config.reduce_motion = !self.config.reduce_motion;
                self.persist_session();
                if let Some(view) = self.tabs[self.active].view.as_ref() {
                    let _ = view.reload();
                }
                self.render_chrome();
            }
            "toggle_background_pause" => {
                self.config.pause_media_when_unfocused = !self.config.pause_media_when_unfocused;
                self.persist_session();
                self.render_chrome();
            }
            "toggle_adblock" => {
                let url = self.tabs[self.active].url.clone();
                if let Some((host, enabled)) = self.blocker.toggle_for_url(&url) {
                    if enabled {
                        self.config
                            .adblock_disabled_hosts
                            .retain(|item| item != &host);
                    } else if !self.config.adblock_disabled_hosts.contains(&host) {
                        self.config.adblock_disabled_hosts.push(host);
                    }
                    self.persist_session();
                    if let Some(view) = self.tabs[self.active].view.as_ref() {
                        let _ = view.reload();
                    }
                    self.render_chrome();
                }
            }
            "cancel_download" => {
                if let Some(id) = command.get("id").and_then(Value::as_u64) {
                    self.cancel_download(id);
                }
            }
            "open_download" => {
                if let Some(id) = command.get("id").and_then(Value::as_u64) {
                    self.open_download(id, false);
                }
            }
            "show_download" => {
                if let Some(id) = command.get("id").and_then(Value::as_u64) {
                    self.open_download(id, true);
                }
            }
            "clear_downloads" => {
                self.downloads
                    .retain(|item| item.status == "downloading" || item.status == "cancelling");
                self.persist_session();
                self.render_chrome();
            }
            "downloads_panel" => {
                self.downloads_panel_open = command
                    .get("open")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.resize_views();
            }
            "set_default_browser" => {
                if let Err(error) = default_browser::register_and_open_settings() {
                    eprintln!("No se pudo registrar CRONI: {error:#}");
                }
            }
            "check_update" => {
                if updater::is_configured() && self.update_status != "checking" {
                    self.update_status = "checking".into();
                    self.render_chrome();
                    updater::start_update_check(self.proxy.clone());
                }
            }
            "install_update" => {
                let Some(path) = self.update_path.clone() else {
                    return;
                };
                let Some(sha256) = self.update_sha256.clone() else {
                    return;
                };
                self.update_status = "installing".into();
                self.render_chrome();
                match updater::stage_and_launch(&path, &sha256) {
                    Ok(()) => {
                        self.persist_session();
                        event_loop.exit();
                    }
                    Err(error) => {
                        self.update_status = "failed".into();
                        eprintln!("No se pudo actualizar CRONI: {error:#}");
                        self.render_chrome();
                    }
                }
            }
            "window_minimize" => {
                if let Some(window) = self.window.as_ref() {
                    window.set_minimized(true);
                }
            }
            "window_toggle_maximize" => {
                if let Some(window) = self.window.as_ref() {
                    window.set_maximized(!window.is_maximized());
                    self.render_chrome();
                }
            }
            "window_drag" => {
                if let Some(window) = self.window.as_ref() {
                    let _ = window.drag_window();
                }
            }
            "window_close" => {
                self.persist_session();
                event_loop.exit();
            }
            _ => {}
        }
    }

    fn cancel_download(&mut self, id: u64) {
        #[cfg(target_os = "windows")]
        match downloads::cancel(&self.download_operations, id) {
            Ok(true) => {
                if let Some(item) = self.downloads.iter_mut().find(|item| item.id == id) {
                    item.status = "cancelling".into();
                }
                self.render_chrome();
            }
            Ok(false) => {}
            Err(error) => eprintln!("No se pudo cancelar la descarga: {error:#}"),
        }
    }

    fn open_download(&self, id: u64, show_in_folder: bool) {
        let Some(item) = self.downloads.iter().find(|item| item.id == id) else {
            return;
        };
        if item.path.is_empty() {
            return;
        }
        #[cfg(target_os = "windows")]
        {
            if show_in_folder && reveal_in_explorer(&item.path) {
                return;
            }
            let mut command = std::process::Command::new("explorer.exe");
            if show_in_folder {
                // Explorer treats the switch and the quoted path as separate arguments.
                command.arg("/select,").arg(&item.path);
            } else {
                command.arg(&item.path);
            }
            if let Err(error) = command.spawn() {
                eprintln!("No se pudo abrir la descarga: {error}");
            }
        }
    }

    fn with_active_view(&self, operation: impl FnOnce(&WebView) -> wry::Result<()>) {
        if let Some(view) = self.tabs[self.active].view.as_ref() {
            if let Err(error) = operation(view) {
                eprintln!("Operación de navegación fallida: {error}");
            }
        }
    }

    fn navigate_with(&self, operation: impl FnOnce(&WebView) -> wry::Result<()>) {
        self.with_active_view(|view| {
            view.evaluate_script(blocker::PAUSE_MEDIA_SCRIPT)?;
            operation(view)
        });
    }

    fn toggle_bookmark(&mut self) {
        let url = self.tabs[self.active].url.clone();
        if let Some(index) = self
            .config
            .bookmarks
            .iter()
            .position(|item| item.url == url)
        {
            self.config.bookmarks.remove(index);
        } else {
            self.add_active_bookmark();
            return;
        }
        self.persist_session();
        self.render_chrome();
    }

    fn add_active_bookmark(&mut self) {
        let url = self.tabs[self.active].url.clone();
        if !self.config.bookmarks.iter().any(|item| item.url == url) {
            self.config.bookmarks.push(Bookmark {
                title: self.tabs[self.active].title.clone(),
                url,
            });
        }
        self.persist_session();
        self.render_chrome();
    }

    fn discard_expired_tabs(&mut self, force: bool) {
        let now = Instant::now();
        let timeout = self.discard_timeout();
        let mut changed = false;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if index != self.active
                && tab.view.is_some()
                && (force || now.duration_since(tab.last_active) >= timeout)
            {
                // Dropping the WebView releases the renderer/document state. URL, title,
                // cookies and disk-backed site data remain available for restoration.
                tab.view = None;
                changed = true;
            }
        }
        if changed {
            self.render_chrome();
        }
    }

    fn next_discard_deadline(&self) -> Option<Instant> {
        let timeout = self.discard_timeout();
        self.tabs
            .iter()
            .enumerate()
            .filter(|(index, tab)| *index != self.active && tab.view.is_some())
            .map(|(_, tab)| tab.last_active + timeout)
            .min()
    }

    fn discard_timeout(&self) -> Duration {
        Duration::from_secs(self.config.discard_after_minutes.max(1) * 60)
    }

    fn render_chrome(&self) {
        let Some(chrome) = self.chrome.as_ref() else {
            return;
        };
        let active = &self.tabs[self.active];
        let state = ChromeState {
            tabs: self
                .tabs
                .iter()
                .map(|tab| TabState {
                    id: tab.id,
                    title: &tab.title,
                    suspended: tab.view.is_none(),
                })
                .collect(),
            active_id: active.id,
            address: &active.url,
            bookmarked: self
                .config
                .bookmarks
                .iter()
                .any(|item| item.url == active.url),
            bookmarks: &self.config.bookmarks,
            downloads: &self.downloads,
            discard_after_minutes: self.config.discard_after_minutes,
            maximized: self
                .window
                .as_ref()
                .map(Window::is_maximized)
                .unwrap_or(false),
            loading: active.loading,
            blocked_count: self.blocker.blocked_count(),
            adblock_enabled: self.blocker.is_enabled_for_url(&active.url),
            adblock_status: self.blocker.status(),
            ultra_light_mode: self.config.ultra_light_mode,
            reduce_motion: self.config.reduce_motion,
            pause_media_when_unfocused: self.config.pause_media_when_unfocused,
            app_version: updater::APP_VERSION,
            update_configured: updater::is_configured(),
            update_status: &self.update_status,
            update_version: self.update_version.as_deref(),
            update_ready: self.update_path.is_some() && self.update_sha256.is_some(),
        };
        #[cfg(target_os = "windows")]
        chrome.render(&state);
        #[cfg(not(target_os = "windows"))]
        if let Ok(json) = serde_json::to_string(&state) {
            let _ = chrome.evaluate_script(&format!("window.renderState({json})"));
        }
    }

    fn resize_views(&self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if let Some(chrome) = self.chrome.as_ref() {
            #[cfg(target_os = "windows")]
            {
                chrome.set_visible(!self.content_fullscreen);
                if !self.content_fullscreen {
                    chrome.resize(window.inner_size().width, window.scale_factor());
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = chrome.set_visible(!self.content_fullscreen);
                let _ = chrome.set_bounds(toolbar_bounds(window, self.downloads_panel_open));
                keep_chrome_above_content(chrome);
            }
        }
        let bounds = content_bounds(window, self.content_fullscreen);
        for tab in &self.tabs {
            if let Some(view) = tab.view.as_ref() {
                let _ = view.set_bounds(bounds);
                #[cfg(target_os = "windows")]
                unsafe {
                    let _ = view.controller().NotifyParentWindowPositionChanged();
                }
            }
        }
    }

    fn persist_session(&mut self) {
        self.config.restore_urls = self.tabs.iter().map(|tab| tab.url.clone()).collect();
        self.config.downloads = self
            .downloads
            .iter()
            .rev()
            .take(30)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|item| item.to_stored())
            .collect();
        if let Err(error) = self.config.save() {
            eprintln!("No se pudo guardar la sesión: {error}");
        }
    }

    fn set_content_fullscreen(&mut self, tab_id: u64, fullscreen: bool) {
        if self.tabs.get(self.active).map(|tab| tab.id) != Some(tab_id)
            || self.content_fullscreen == fullscreen
        {
            return;
        }
        self.content_fullscreen = fullscreen;
        if let Some(window) = self.window.as_ref() {
            let target = fullscreen.then(|| Fullscreen::Borderless(window.current_monitor()));
            window.set_fullscreen(target);
        }
        self.resize_views();
    }
}

impl ApplicationHandler<UserEvent> for BrowserApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            if let Err(error) = self.initialize(event_loop) {
                eprintln!("No se pudo iniciar CRONI: {error:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(Window::id) != Some(window_id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.persist_session();
                event_loop.exit();
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.resize_views();
                self.render_chrome();
            }
            WindowEvent::Focused(focused) => {
                self.window_is_focused = focused;
                for (index, tab) in self.tabs.iter().enumerate() {
                    if let Some(view) = tab.view.as_ref() {
                        set_memory_level(view, focused && index == self.active);
                        if !focused
                            && !self.content_fullscreen
                            && self.config.pause_media_when_unfocused
                        {
                            let _ = view.evaluate_script(blocker::PAUSE_MEDIA_SCRIPT);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::ChromeCommand(command) => self.handle_chrome_command(&command, event_loop),
            UserEvent::PageLoadChanged {
                tab_id,
                url,
                loading,
            } => {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                    tab.url = url.clone();
                    tab.loading = loading;
                    self.persist_session();
                    self.render_chrome();
                }
                if !loading {
                    let script = self.blocker.cosmetic_script(&url);
                    if let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) {
                        if let (Some(view), Some(script)) = (tab.view.as_ref(), script) {
                            let _ = view.evaluate_script(&script);
                        }
                    }
                    if self.refresh_surface_after_load == Some(tab_id) {
                        self.refresh_surface_after_load = None;
                        if let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) {
                            if let Some(view) = tab.view.as_ref() {
                                refresh_view_surface(view);
                            }
                        }
                    }
                }
            }
            UserEvent::LocationChanged { tab_id, url } => {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                    if tab.url != url {
                        tab.url = url;
                        self.persist_session();
                        self.render_chrome();
                    }
                }
            }
            UserEvent::TitleChanged { tab_id, title } => {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                    tab.title = if title.trim().is_empty() {
                        title_from_url(&tab.url)
                    } else {
                        title
                    };
                    self.render_chrome();
                }
            }
            UserEvent::OpenInNewTab(url) => self.add_tab(&url, true),
            UserEvent::DownloadStarted {
                id,
                name,
                url,
                path,
                received,
                total,
            } => {
                self.downloads.push(DownloadItem {
                    id,
                    name,
                    url,
                    path,
                    received,
                    total,
                    status: "downloading".into(),
                });
                if self.downloads.len() > 30 {
                    self.downloads.remove(0);
                }
                self.persist_session();
                self.render_chrome();
            }
            UserEvent::DownloadProgress {
                id,
                received,
                total,
                status,
            } => {
                if let Some(item) = self.downloads.iter_mut().find(|item| item.id == id) {
                    item.received = received;
                    item.total = total;
                    let finished = status != "downloading";
                    item.status = status;
                    if finished {
                        self.persist_session();
                    }
                    self.render_chrome();
                }
            }
            UserEvent::UpdateReady {
                version,
                path,
                sha256,
            } => {
                self.update_status = "ready".into();
                self.update_version = Some(version);
                self.update_path = Some(path);
                self.update_sha256 = Some(sha256);
                self.render_chrome();
            }
            UserEvent::UpdateCurrent => {
                self.update_status = "current".into();
                self.update_version = None;
                self.update_path = None;
                self.update_sha256 = None;
                self.render_chrome();
            }
            UserEvent::UpdateFailed(message) => {
                self.update_status = "failed".into();
                self.update_path = None;
                self.update_sha256 = None;
                eprintln!("No se pudo buscar la actualización: {message}");
                self.render_chrome();
            }
            UserEvent::ContentFullscreenChanged { tab_id, fullscreen } => {
                self.set_content_fullscreen(tab_id, fullscreen);
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.discard_expired_tabs(false);
        event_loop.set_control_flow(match self.next_discard_deadline() {
            Some(deadline) => ControlFlow::WaitUntil(deadline),
            None => ControlFlow::Wait,
        });
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        self.discard_expired_tabs(true);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.persist_session();
    }
}

#[cfg(not(target_os = "windows"))]
const DOWNLOAD_PANEL_HEIGHT_LOGICAL: f64 = 320.0;

#[cfg(not(target_os = "windows"))]
fn toolbar_height(window: &Window, downloads_panel_open: bool) -> u32 {
    let logical_height = TOOLBAR_HEIGHT_LOGICAL
        + if downloads_panel_open {
            DOWNLOAD_PANEL_HEIGHT_LOGICAL
        } else {
            0.0
        };
    (logical_height * window.scale_factor()).round() as u32
}

#[cfg(not(target_os = "windows"))]
fn toolbar_bounds(window: &Window, downloads_panel_open: bool) -> Rect {
    let size = window.inner_size();
    Rect {
        position: PhysicalPosition::new(0, 0).into(),
        size: PhysicalSize::new(size.width, toolbar_height(window, downloads_panel_open)).into(),
    }
}

fn content_bounds(window: &Window, fullscreen: bool) -> Rect {
    let size = window.inner_size();
    let top = if fullscreen {
        0
    } else {
        (TOOLBAR_HEIGHT_LOGICAL * window.scale_factor()).round() as u32
    };
    let top = top.min(size.height);
    Rect {
        position: PhysicalPosition::new(0, top as i32).into(),
        size: PhysicalSize::new(size.width, size.height.saturating_sub(top)).into(),
    }
}

#[cfg(target_os = "windows")]
fn attach_fullscreen_handler(
    view: &WebView,
    proxy: EventLoopProxy<UserEvent>,
    tab_id: u64,
) -> Result<()> {
    use webview2_com::ContainsFullScreenElementChangedEventHandler;
    use windows::core::BOOL;

    let webview = view.webview();
    let handler =
        ContainsFullScreenElementChangedEventHandler::create(Box::new(move |sender, _| {
            let Some(sender) = sender else {
                return Ok(());
            };
            let mut contains = BOOL(0);
            unsafe { sender.ContainsFullScreenElement(&mut contains)? };
            let _ = proxy.send_event(UserEvent::ContentFullscreenChanged {
                tab_id,
                fullscreen: contains.as_bool(),
            });
            Ok(())
        }));
    unsafe {
        webview.add_ContainsFullScreenElementChanged(&handler, &mut 0)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn attach_location_changed_handler(
    view: &WebView,
    proxy: EventLoopProxy<UserEvent>,
    tab_id: u64,
    page_url: Arc<RwLock<String>>,
) -> Result<()> {
    use webview2_com::{take_pwstr, SourceChangedEventHandler};
    use windows::core::PWSTR;

    let webview = view.webview();
    let handler = SourceChangedEventHandler::create(Box::new(move |sender, _| {
        let Some(sender) = sender else {
            return Ok(());
        };
        let mut source = PWSTR::null();
        unsafe { sender.Source(&mut source)? };
        let url = take_pwstr(source);
        if blocker::is_navigation_allowed(&url) {
            *page_url.write().unwrap_or_else(|lock| lock.into_inner()) = url.clone();
            let _ = proxy.send_event(UserEvent::LocationChanged { tab_id, url });
        }
        Ok(())
    }));
    unsafe {
        webview.add_SourceChanged(&handler, &mut 0)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn refresh_view_surface(view: &WebView) {
    let _ = view.set_visible(false);
    unsafe {
        let _ = view.controller().NotifyParentWindowPositionChanged();
    }
    let _ = view.set_visible(true);
    let _ = view.focus();
}

#[cfg(not(target_os = "windows"))]
fn refresh_view_surface(_view: &WebView) {}

fn app_icon() -> Option<Icon> {
    Icon::from_rgba(
        include_bytes!("../assets/app_icon_64.rgba").to_vec(),
        64,
        64,
    )
    .ok()
}

#[cfg(not(target_os = "windows"))]
fn keep_chrome_above_content(_chrome: &WebView) {}

#[cfg(target_os = "windows")]
fn reveal_in_explorer(path: &str) -> bool {
    let path = HSTRING::from(path);
    let item_id = unsafe { ILCreateFromPathW(&path) };
    if item_id.is_null() {
        return false;
    }
    let result = unsafe { SHOpenFolderAndSelectItems(item_id, None, 0) };
    unsafe { ILFree(Some(item_id)) };
    result.is_ok()
}

#[cfg(target_os = "windows")]
fn set_memory_level(view: &WebView, active: bool) {
    let level = if active {
        MemoryUsageLevel::Normal
    } else {
        MemoryUsageLevel::Low
    };
    let _ = view.set_memory_usage_level(level);
}

#[cfg(not(target_os = "windows"))]
fn set_memory_level(_view: &WebView, _active: bool) {}

fn title_from_url(raw: &str) -> String {
    url::Url::parse(raw)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Nueva pestaña".to_string())
}
