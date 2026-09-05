(() => {
  'use strict';
  // A blob URL belongs to its creator. Opening it in a separately-created tab
  // loses that context. Let the engine download explicit links in this frame.
  // No fetch/base64 bridge: cookies, streaming and download safety stay native.
  document.addEventListener('click', event => {
    const link = event.composedPath().find(node =>
      node instanceof HTMLAnchorElement && node.hasAttribute('download'));
    if (!link || !/^(blob|data):/i.test(link.href)) return;
    link.target = '_self';
  }, true);
})();
