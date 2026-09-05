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
  const youtubeHost = location.hostname === 'youtube.com' || location.hostname.endsWith('.youtube.com');
  const skipSelectors = '.ytp-ad-skip-button, .ytp-ad-skip-button-modern, .ytp-skip-ad-button, [id^="skip-button"] button';
  const youtubeAdSelectors = 'ytd-display-ad-renderer, ytd-promoted-sparkles-web-renderer, ytd-action-companion-ad-renderer, .ytp-ad-overlay-container';
  const visitMatches = (root, selectors, visit) => {
    if (root.matches?.(selectors)) visit(root);
    root.querySelectorAll(selectors).forEach(visit);
  };
  const handleYouTubeAds = root => {
    if (!youtubeHost) return;
    visitMatches(root, skipSelectors, button => {
      try { button.click(); } catch (_) {}
    });
    visitMatches(root, youtubeAdSelectors, node => node.remove());
  };
  document.addEventListener('yt-navigate-finish', () => {
    if (!document.hidden) handleYouTubeAds(document);
  }, true);

  // Lightweight fallback for inline ad containers. Network filtering is done natively
  // before resources are downloaded by the Brave adblock-rust engine.
  const selectors = [
    // Do not match arbitrary ad-* classes: YouTube's player uses ad-showing.
    '[id^="google_ads"]',
    '[data-ad-client]', '[data-ad-slot]', '.adsbygoogle',
    'iframe[src*="doubleclick.net"]', 'iframe[src*="googlesyndication.com"]'
  ].join(',');
  const clean = root => {
    if (!root || !root.querySelectorAll) return;
    if (root.matches && root.matches(selectors)) {
      root.remove();
      return;
    }
    root.querySelectorAll(selectors).forEach(node => node.remove());
    handleYouTubeAds(root);
  };
  const observer = new MutationObserver(records => {
    // Scan inserted subtrees, not the entire YouTube document on every mutation.
    const roots = new Set();
    records.forEach(record => record.addedNodes.forEach(node => {
      if (node.nodeType === 1 && node.isConnected) roots.add(node);
    }));
    for (const root of roots) {
      let parent = root.parentElement;
      while (parent && !roots.has(parent)) parent = parent.parentElement;
      if (!parent && root.isConnected) clean(root);
    }
  });
  let observing = false;
  const syncObserver = () => {
    if (document.hidden) {
      observer.disconnect();
      observing = false;
      return;
    }
    if (observing || !document.documentElement) return;
    clean(document);
    observer.observe(document.documentElement, { childList: true, subtree: true });
    observing = true;
  };
  document.addEventListener('visibilitychange', syncObserver);
  window.addEventListener('pagehide', () => {
    observer.disconnect();
    observing = false;
  });
  window.addEventListener('pageshow', syncObserver);
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', syncObserver, { once: true });
  } else {
    syncObserver();
  }
})();
