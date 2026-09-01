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

  const youtubeHost = location.hostname === 'youtube.com' || location.hostname.endsWith('.youtube.com');
  let youtubePassScheduled = false;
  const handleYouTubeAds = () => {
    youtubePassScheduled = false;
    if (!youtubeHost) return;
    document.querySelectorAll(
      '.ytp-ad-skip-button, .ytp-ad-skip-button-modern, .ytp-skip-ad-button, [id^="skip-button"] button'
    ).forEach(button => {
      try { button.click(); } catch (_) {}
    });
    document.querySelectorAll(
      'ytd-display-ad-renderer, ytd-promoted-sparkles-web-renderer, ytd-action-companion-ad-renderer, .ytp-ad-overlay-container'
    ).forEach(node => node.remove());

    const player = document.querySelector('.html5-video-player');
    const video = player && player.querySelector('video');
    if (!video) return;
    if (player.classList.contains('ad-showing')) {
      if (!video.__croniAdState) {
        video.__croniAdState = { muted: video.muted, rate: video.playbackRate };
      }
      video.muted = true;
      video.playbackRate = 16;
      if (Number.isFinite(video.duration) && video.duration > 0) {
        try { video.currentTime = Math.max(0, video.duration - 0.05); } catch (_) {}
      }
    } else if (video.__croniAdState) {
      const previous = video.__croniAdState;
      delete video.__croniAdState;
      video.muted = previous.muted;
      video.playbackRate = previous.rate;
    }
  };
  const scheduleYouTubePass = () => {
    if (!youtubeHost || youtubePassScheduled) return;
    youtubePassScheduled = true;
    requestAnimationFrame(handleYouTubeAds);
  };
  document.addEventListener('yt-navigate-finish', scheduleYouTubePass, true);

  // Lightweight fallback for inline ad containers. Network filtering is done natively
  // before resources are downloaded by the Brave adblock-rust engine.
  const selectors = [
    '[id^="google_ads"]', '[class*=" ad-"]', '[class^="ad-"]',
    '[data-ad-client]', '[data-ad-slot]', '.adsbygoogle',
    'iframe[src*="doubleclick.net"]', 'iframe[src*="googlesyndication.com"]'
  ].join(',');
  const clean = root => {
    if (!root || !root.querySelectorAll) return;
    if (root.matches && root.matches(selectors)) root.remove();
    root.querySelectorAll(selectors).forEach(node => node.remove());
  };
  document.addEventListener('DOMContentLoaded', () => {
    clean(document);
    scheduleYouTubePass();
    new MutationObserver(records => {
      records.forEach(record => record.addedNodes.forEach(clean));
      scheduleYouTubePass();
    }).observe(document.documentElement, { childList: true, subtree: true });
  }, { once: true });
})();
