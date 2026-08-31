(() => {
  'use strict';
  const blocked = [
    'doubleclick.net', 'googlesyndication.com', 'googleadservices.com',
    'google-analytics.com', 'adnxs.com', 'adsrvr.org', 'adform.net',
    'advertising.com', 'amazon-adsystem.com', 'casalemedia.com',
    'demdex.net', 'media.net', 'moatads.com', 'openx.net', 'pubmatic.com',
    'quantserve.com', 'rlcdn.com', 'rubiconproject.com', 'scorecardresearch.com',
    'sharethrough.com', 'smartadserver.com',
    'taboola.com', 'outbrain.com', 'criteo.com', 'criteo.net',
    'hotjar.com', 'clarity.ms', 'yieldmo.com'
  ];
  const denied = value => {
    try {
      const host = new URL(String(value), location.href).hostname.toLowerCase();
      return blocked.some(domain => host === domain || host.endsWith('.' + domain));
    } catch (_) { return false; }
  };

  // YouTube's ad player shares state with its main player. Blocking only its
  // tracking/control requests can leave the media stream playing indefinitely.
  // On YouTube we keep lifecycle protection but let the site manage its network.
  const host = location.hostname.toLowerCase();
  const siteManagesAds = host === 'youtube.com' || host.endsWith('.youtube.com');

  const pauseMedia = root => {
    const scope = root && root.querySelectorAll ? root : document;
    if (root && root.matches && root.matches('audio, video')) {
      try { root.pause(); } catch (_) {}
    }
    scope.querySelectorAll('audio, video').forEach(media => {
      try { media.pause(); } catch (_) {}
    });
  };

  // Covers full navigations, browser back/forward and YouTube's SPA router.
  window.addEventListener('pagehide', () => pauseMedia(document), true);
  window.addEventListener('popstate', () => pauseMedia(document), true);
  document.addEventListener('yt-navigate-start', () => pauseMedia(document), true);

  const nativeFetch = window.fetch;
  if (nativeFetch && !siteManagesAds) {
    window.fetch = function(input, init) {
      const target = input && input.url ? input.url : input;
      return denied(target)
        ? Promise.reject(new TypeError('Blocked by CRONI'))
        : nativeFetch.call(this, input, init);
    };
  }

  if (!siteManagesAds) {
    const nativeOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function(method, url) {
      this.__navegadirBlocked = denied(url);
      return nativeOpen.apply(this, arguments);
    };
    const nativeSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function() {
      if (this.__navegadirBlocked) { this.abort(); return; }
      return nativeSend.apply(this, arguments);
    };
  }

  if (navigator.sendBeacon && !siteManagesAds) {
    const nativeBeacon = navigator.sendBeacon.bind(navigator);
    navigator.sendBeacon = (url, data) => denied(url) ? false : nativeBeacon(url, data);
  }

  const selectors = [
    '[id^="google_ads"]', '[class*=" ad-"]', '[class^="ad-"]',
    '[data-ad-client]', '[data-ad-slot]', '.adsbygoogle',
    'iframe[src*="doubleclick.net"]', 'iframe[src*="googlesyndication.com"]'
  ].join(',');
  const clean = root => {
    if (!root || !root.querySelectorAll) return;
    root.querySelectorAll(selectors).forEach(node => node.remove());
  };
  document.addEventListener('DOMContentLoaded', () => {
    if (siteManagesAds) return;
    clean(document);
    new MutationObserver(records => records.forEach(record =>
      record.addedNodes.forEach(clean)
    )).observe(document.documentElement, { childList: true, subtree: true });
  }, { once: true });
})();
