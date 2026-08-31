(() => {
  'use strict';

  const pauseMedia = root => {
    const scope = root && root.querySelectorAll ? root : document;
    if (root && root.matches && root.matches('audio, video')) {
      try { root.pause(); root.removeAttribute('autoplay'); } catch (_) {}
    }
    scope.querySelectorAll('audio, video').forEach(media => {
      try { media.pause(); media.removeAttribute('autoplay'); } catch (_) {}
    });
  };

  // Prevents audio from a previous YouTube SPA page or an ad from surviving navigation.
  window.addEventListener('pagehide', () => pauseMedia(document), true);
  window.addEventListener('beforeunload', () => pauseMedia(document), true);
  window.addEventListener('popstate', () => pauseMedia(document), true);
  document.addEventListener('yt-navigate-start', () => pauseMedia(document), true);
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) pauseMedia(document);
  }, true);

  // Lightweight fallback for inline ad containers. Network filtering is done natively
  // before resources are downloaded by the Brave adblock-rust engine.
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
    clean(document);
    new MutationObserver(records => records.forEach(record =>
      record.addedNodes.forEach(clean)
    )).observe(document.documentElement, { childList: true, subtree: true });
  }, { once: true });
})();
