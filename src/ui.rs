use serde::Serialize;

pub const TOOLBAR_HEIGHT_LOGICAL: f64 = 144.0;
#[cfg(not(target_os = "windows"))]
pub const CHROME_HTML: &str = include_str!("../assets/chrome.html");

#[derive(Serialize)]
pub struct ChromeState<'a> {
    pub tabs: Vec<TabState<'a>>,
    pub active_id: u64,
    pub address: &'a str,
    pub bookmarked: bool,
    pub bookmarks: &'a [crate::config::Bookmark],
    pub downloads: &'a [crate::downloads::DownloadItem],
    pub discard_after_minutes: u64,
    pub maximized: bool,
    pub loading: bool,
    pub blocked_count: u64,
    pub adblock_enabled: bool,
    pub adblock_status: &'static str,
    pub ultra_light_mode: bool,
    pub reduce_motion: bool,
    pub pause_media_when_unfocused: bool,
    pub app_version: &'static str,
    pub update_configured: bool,
    pub update_status: &'a str,
    pub update_version: Option<&'a str>,
    pub update_ready: bool,
}

#[derive(Serialize)]
pub struct TabState<'a> {
    pub id: u64,
    pub title: &'a str,
    pub suspended: bool,
}
