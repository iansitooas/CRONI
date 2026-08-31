use url::Url;

// Intentionally compact: a huge filter list would work against the memory budget.
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

pub const INITIALIZATION_SCRIPT: &str = include_str!("../assets/content_filter.js");

/// Stops media before a tab is hidden or its current document is replaced.
/// Keeping this separate also lets native navigation buttons stop SPA players.
pub const PAUSE_MEDIA_SCRIPT: &str = r#"
    document.querySelectorAll('audio, video').forEach(media => {
        try { media.pause(); } catch (_) {}
    });
"#;

pub fn is_blocked_url(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
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

#[cfg(target_os = "windows")]
pub fn attach_native_protections(view: &wry::WebView) -> anyhow::Result<()> {
    use anyhow::Context;
    use webview2_com::{
        take_pwstr, Microsoft::Web::WebView2::Win32::*, WebResourceRequestedEventHandler,
    };
    use windows::core::{HSTRING, PWSTR};
    use wry::WebViewExtWindows;

    let webview = view.webview();
    let environment = view.environment();

    // Content pages never need access to CRONI's native IPC or host objects.
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
        let request = unsafe { args.Request()? };
        let mut uri = PWSTR::null();
        unsafe { request.Uri(&mut uri)? };
        let uri = take_pwstr(uri);

        if is_blocked_url(&uri) {
            let reason = HSTRING::from("No Content");
            let headers =
                HSTRING::from("Cache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\n");
            let response =
                unsafe { environment.CreateWebResourceResponse(None, 204, &reason, &headers)? };
            unsafe { args.SetResponse(&response)? };
        }
        Ok(())
    }));
    unsafe {
        webview.add_WebResourceRequested(&handler, &mut 0)?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn attach_native_protections(_view: &wry::WebView) -> anyhow::Result<()> {
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
    fn permits_only_web_navigation_and_blank_pages() {
        assert!(is_navigation_allowed("https://example.com"));
        assert!(is_navigation_allowed("http://localhost:3000"));
        assert!(is_navigation_allowed("about:blank"));
        assert!(!is_navigation_allowed("file:///C:/Users/test/secreto.txt"));
        assert!(!is_navigation_allowed("javascript:alert(1)"));
        assert!(!is_navigation_allowed("data:text/html,malicioso"));
    }
}
