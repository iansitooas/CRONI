use crate::{config::app_data_dir, updater};
use adblock::{lists::ParseOptions, request::Request, Engine, FilterSet};
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc, RwLock,
    },
    thread,
    time::Duration,
};
use url::Url;

const BLOCKED_HOSTS: &[&str] = &[
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "google-analytics.com",
    "adnxs.com",
    "adsrvr.org",
    "adform.net",
    "advertising.com",
    "amazon-adsystem.com",
    "casalemedia.com",
    "demdex.net",
    "media.net",
    "moatads.com",
    "openx.net",
    "pubmatic.com",
    "quantserve.com",
    "rlcdn.com",
    "rubiconproject.com",
    "scorecardresearch.com",
    "sharethrough.com",
    "smartadserver.com",
    "taboola.com",
    "outbrain.com",
    "criteo.com",
    "criteo.net",
    "hotjar.com",
    "clarity.ms",
    "yieldmo.com",
];

const FILTER_SOURCES: &[&str] = &[
    "https://easylist.to/easylist/easylist.txt",
    "https://easylist.to/easylist/easyprivacy.txt",
    "https://raw.githubusercontent.com/brave/adblock-lists/master/brave-lists/brave-specific.txt",
    "https://raw.githubusercontent.com/brave/adblock-lists/master/brave-lists/brave-firstparty.txt",
    "https://raw.githubusercontent.com/brave/adblock-lists/master/brave-unbreak.txt",
];
const FILTER_UPDATE_INTERVAL: Duration = Duration::from_secs(48 * 60 * 60);
const MAX_FILTER_BYTES: usize = 12 * 1024 * 1024;

pub const INITIALIZATION_SCRIPT: &str = include_str!("../assets/content_filter.js");

pub const PERFORMANCE_SCRIPT: &str = r#"
(() => {
  'use strict';
  const style = document.createElement('style');
  style.id = 'croni-reduced-motion';
  style.textContent = `
    *, *::before, *::after {
      animation-duration: 0.001ms !important;
      animation-iteration-count: 1 !important;
      transition-duration: 0.001ms !important;
      scroll-behavior: auto !important;
    }
  `;
  const install = () => (document.head || document.documentElement).appendChild(style);
  if (document.documentElement) install();
  else document.addEventListener('DOMContentLoaded', install, { once: true });
})();
"#;

pub const PAUSE_MEDIA_SCRIPT: &str = r#"
    document.querySelectorAll('audio, video').forEach(media => {
        try { media.pause(); media.removeAttribute('autoplay'); } catch (_) {}
    });
"#;

#[derive(Clone)]
pub struct BlockerManager {
    engine: Arc<RwLock<Engine>>,
    disabled_hosts: Arc<RwLock<HashSet<String>>>,
    blocked_count: Arc<AtomicU64>,
    status: Arc<AtomicU8>,
}

impl BlockerManager {
    pub fn new(disabled_hosts: &[String]) -> Self {
        Self {
            engine: Arc::new(RwLock::new(
                load_cached_engine().unwrap_or_else(starter_engine),
            )),
            disabled_hosts: Arc::new(RwLock::new(
                disabled_hosts
                    .iter()
                    .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
                    .filter(|host| !host.is_empty())
                    .collect(),
            )),
            blocked_count: Arc::new(AtomicU64::new(0)),
            status: Arc::new(AtomicU8::new(if engine_cache_is_fresh() { 2 } else { 0 })),
        }
    }

    pub fn start_filter_update(&self) {
        if engine_cache_is_fresh() || self.status.swap(1, Ordering::Relaxed) == 1 {
            return;
        }
        let manager = self.clone();
        thread::spawn(move || match download_engine() {
            Ok(engine) => {
                let serialized = engine.serialize();
                if let Err(error) = save_engine_cache(&serialized) {
                    eprintln!("No se pudo guardar la lista de bloqueo: {error}");
                }
                *manager
                    .engine
                    .write()
                    .unwrap_or_else(|lock| lock.into_inner()) = engine;
                manager.status.store(2, Ordering::Relaxed);
            }
            Err(error) => {
                manager.status.store(3, Ordering::Relaxed);
                eprintln!("No se pudieron actualizar los filtros: {error:#}");
            }
        });
    }

    pub fn blocked_count(&self) -> u64 {
        self.blocked_count.load(Ordering::Relaxed)
    }

    pub fn status(&self) -> &'static str {
        match self.status.load(Ordering::Relaxed) {
            1 => "updating",
            2 => "ready",
            3 => "failed",
            _ => "starter",
        }
    }

    pub fn is_enabled_for_url(&self, raw: &str) -> bool {
        let Some(host) = host_from_url(raw) else {
            return true;
        };
        !self
            .disabled_hosts
            .read()
            .unwrap_or_else(|lock| lock.into_inner())
            .contains(&host)
    }

    pub fn toggle_for_url(&self, raw: &str) -> Option<(String, bool)> {
        let host = host_from_url(raw)?;
        let mut disabled = self
            .disabled_hosts
            .write()
            .unwrap_or_else(|lock| lock.into_inner());
        let enabled = if disabled.remove(&host) {
            true
        } else {
            disabled.insert(host.clone());
            false
        };
        Some((host, enabled))
    }

    pub fn cosmetic_script(&self, raw: &str) -> Option<String> {
        if !self.is_enabled_for_url(raw) {
            return None;
        }
        let engine = self.engine.read().ok()?;
        let resources = engine.url_cosmetic_resources(raw);
        let mut selectors = resources.hide_selectors.into_iter().collect::<Vec<_>>();
        selectors.sort_unstable();
        selectors.truncate(2048);
        if selectors.is_empty() {
            return None;
        }
        let css = format!("{}{{display:none!important}}", selectors.join(","));
        if css.len() > 256 * 1024 {
            return None;
        }
        let css = serde_json::to_string(&css).ok()?;
        Some(format!(
            r#"(() => {{
            let style = document.getElementById('croni-site-filters');
            if (!style) {{ style = document.createElement('style'); style.id = 'croni-site-filters'; (document.head || document.documentElement).appendChild(style); }}
            style.textContent = {css};
        }})()"#
        ))
    }
}

pub fn is_blocked_url(raw: &str) -> bool {
    let Some(host) = host_from_url(raw) else {
        return false;
    };
    BLOCKED_HOSTS.iter().any(|blocked| {
        host == *blocked
            || host
                .strip_suffix(blocked)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

pub fn is_navigation_allowed(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https") || (url.scheme() == "about" && url.path() == "blank")
}

// Renderer-generated downloads must retain their creating document's context.
// This does not allow these schemes as command-line URLs or restored sessions.
pub fn is_webview_navigation_allowed(raw: &str) -> bool {
    is_navigation_allowed(raw)
        || Url::parse(raw).is_ok_and(|url| matches!(url.scheme(), "blob" | "data"))
}

fn host_from_url(raw: &str) -> Option<String> {
    Url::parse(raw)
        .ok()?
        .host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
}

fn starter_rules() -> String {
    let mut rules = String::from("[Adblock Plus 2.0]\n! CRONI starter rules\n");
    for host in BLOCKED_HOSTS {
        rules.push_str("||");
        rules.push_str(host);
        rules.push_str("^\n");
    }
    rules.push_str("##.adsbygoogle\n##[data-ad-client]\n##[data-ad-slot]\n##iframe[src*=\"doubleclick.net\"]\n");
    rules
}

fn starter_engine() -> Engine {
    Engine::new_with_list_text(starter_rules())
}

fn download_engine() -> anyhow::Result<Engine> {
    use anyhow::Context;
    let mut set = FilterSet::new(false);
    set.add_filter_list(starter_rules(), ParseOptions::default());
    for source in FILTER_SOURCES {
        let bytes = updater::http_get(source, MAX_FILTER_BYTES)
            .with_context(|| format!("no se pudo descargar {source}"))?;
        let text = String::from_utf8(bytes).context("una lista de filtros no es UTF-8 válido")?;
        set.add_filter_list(text, ParseOptions::default());
    }
    Ok(Engine::new_with_filter_set(set))
}

fn engine_cache_path() -> PathBuf {
    app_data_dir().join("Adblock").join("engine.dat")
}

fn load_cached_engine() -> Option<Engine> {
    let bytes = fs::read(engine_cache_path()).ok()?;
    let mut engine = Engine::default();
    engine.deserialize(&bytes).ok()?;
    Some(engine)
}

fn engine_cache_is_fresh() -> bool {
    fs::metadata(engine_cache_path())
        .and_then(|metadata| metadata.modified())
        .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
        .is_ok_and(|age| age < FILTER_UPDATE_INTERVAL)
}

fn save_engine_cache(bytes: &[u8]) -> std::io::Result<()> {
    let path = engine_cache_path();
    let parent = path.parent().expect("ruta de caché sin carpeta");
    fs::create_dir_all(parent)?;
    let temporary = parent.join("engine.dat.new");
    let backup = parent.join("engine.dat.old");
    fs::write(&temporary, bytes)?;
    let _ = fs::remove_file(&backup);
    if path.exists() {
        fs::rename(&path, &backup)?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::rename(&backup, &path);
        return Err(error);
    }
    let _ = fs::remove_file(backup);
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn attach_native_protections(
    view: &wry::WebView,
    manager: BlockerManager,
    page_url: Arc<RwLock<String>>,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use webview2_com::{
        take_pwstr, Microsoft::Web::WebView2::Win32::*, WebResourceRequestedEventHandler,
    };
    use windows::core::{HSTRING, PWSTR};
    use wry::WebViewExtWindows;

    let webview = view.webview();
    let environment = view.environment();
    let settings = unsafe { webview.Settings() }
        .context("no se pudo leer la configuración de seguridad de WebView2")?;
    unsafe {
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetIsWebMessageEnabled(false)?;
    }
    let all = HSTRING::from("*");
    unsafe {
        webview.AddWebResourceRequestedFilter(&all, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
    }

    let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let source = page_url
            .read()
            .unwrap_or_else(|lock| lock.into_inner())
            .clone();
        if !manager.is_enabled_for_url(&source) {
            return Ok(());
        }
        let request = unsafe { args.Request()? };
        let mut uri = PWSTR::null();
        let mut method = PWSTR::null();
        unsafe {
            request.Uri(&mut uri)?;
            request.Method(&mut method)?;
        }
        let uri = take_pwstr(uri);
        let method = take_pwstr(method);
        let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL;
        unsafe {
            args.ResourceContext(&mut context)?;
        }
        let blocked = Request::new(&uri, &source, resource_type(context), &method)
            .ok()
            .and_then(|request| {
                manager
                    .engine
                    .read()
                    .ok()
                    .map(|engine| engine.check_network_request(&request).should_block())
            })
            .unwrap_or_else(|| is_blocked_url(&uri));
        if blocked {
            manager.blocked_count.fetch_add(1, Ordering::Relaxed);
            let reason = HSTRING::from("No Content");
            let headers =
                HSTRING::from("Cache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\n");
            let response =
                unsafe { environment.CreateWebResourceResponse(None, 204, &reason, &headers)? };
            unsafe {
                args.SetResponse(&response)?;
            }
        }
        Ok(())
    }));
    unsafe {
        webview.add_WebResourceRequested(&handler, &mut 0)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn resource_type(
    context: webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_WEB_RESOURCE_CONTEXT,
) -> &'static str {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    match context {
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => "document",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST => "xmlhttprequest",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "fetch",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => "websocket",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
        _ => "other",
    }
}

#[cfg(not(target_os = "windows"))]
pub fn attach_native_protections(
    _view: &wry::WebView,
    _manager: BlockerManager,
    _page_url: Arc<RwLock<String>>,
) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_domain_and_subdomain_but_not_lookalike() {
        assert!(is_blocked_url("https://ads.doubleclick.net/pixel"));
        assert!(is_blocked_url("https://doubleclick.net"));
        assert!(!is_blocked_url("https://notdoubleclick.net"));
        assert!(!is_blocked_url("https://mail.google.com"));
    }

    #[test]
    fn starter_engine_blocks_ads_but_not_normal_pages() {
        let engine = starter_engine();
        let ad = Request::new(
            "https://ads.doubleclick.net/pixel",
            "https://example.com",
            "image",
            "get",
        )
        .unwrap();
        let page = Request::new(
            "https://www.youtube.com/watch?v=test",
            "https://www.youtube.com",
            "document",
            "get",
        )
        .unwrap();
        assert!(engine.check_network_request(&ad).should_block());
        assert!(!engine.check_network_request(&page).should_block());
    }

    #[test]
    fn permits_only_web_navigation_and_blank_pages() {
        assert!(is_navigation_allowed("https://example.com"));
        assert!(is_navigation_allowed("http://localhost:3000"));
        assert!(is_navigation_allowed("about:blank"));
        assert!(!is_navigation_allowed("file:///C:/Users/test/secreto.txt"));
        assert!(!is_navigation_allowed("javascript:alert(1)"));
        assert!(!is_navigation_allowed("data:text/html,malicioso"));
    }
}
