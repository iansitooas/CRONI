(() => {
  'use strict';
  const pauseMedia = () => {
    document.querySelectorAll('audio, video').forEach(media => {
      try { media.pause(); media.removeAttribute('autoplay'); } catch (_) {}
    });
  };
  // Media lifecycle is independent of whether the ad blocker is enabled.
  window.addEventListener('pagehide', pauseMedia, true);
  window.addEventListener('beforeunload', pauseMedia, true);
  window.addEventListener('popstate', pauseMedia, true);
  document.addEventListener('yt-navigate-start', pauseMedia, true);
})();
