(() => {
  if (window.top !== window || window.__croniPseudoFullscreenInstalled) return;
  window.__croniPseudoFullscreenInstalled = true;

  const marker = "data-croni-pseudo-fullscreen";
  const rootClass = "croni-pseudo-fullscreen-active";
  let activeElement = null;

  const notifyHost = (fullscreen) => {
    try {
      window.ipc.postMessage(JSON.stringify({
        type: "content_fullscreen",
        fullscreen,
      }));
    } catch (_) {}
  };

  const emitChange = (target) => {
    queueMicrotask(() => {
      target.dispatchEvent(new Event("fullscreenchange", { bubbles: true }));
      target.dispatchEvent(new Event("webkitfullscreenchange", { bubbles: true }));
    });
  };

  const leave = () => {
    const previous = activeElement;
    if (!previous) return Promise.resolve();
    previous.removeAttribute(marker);
    activeElement = null;
    document.documentElement?.classList.remove(rootClass);
    notifyHost(false);
    emitChange(previous);
    return Promise.resolve();
  };

  const enter = (element) => {
    if (!(element instanceof Element)) {
      return Promise.reject(new TypeError("El elemento no admite pantalla completa"));
    }
    if (activeElement && activeElement !== element) {
      activeElement.removeAttribute(marker);
    }
    activeElement = element;
    element.setAttribute(marker, "");
    document.documentElement?.classList.add(rootClass);
    notifyHost(true);
    emitChange(element);
    return Promise.resolve();
  };

  const request = function () {
    return enter(this);
  };

  for (const name of ["requestFullscreen", "webkitRequestFullscreen", "webkitRequestFullScreen"]) {
    try {
      Object.defineProperty(Element.prototype, name, {
        configurable: true,
        writable: true,
        value: request,
      });
    } catch (_) {}
  }

  for (const name of ["exitFullscreen", "webkitExitFullscreen", "webkitCancelFullScreen"]) {
    try {
      Object.defineProperty(document, name, {
        configurable: true,
        writable: true,
        value: leave,
      });
    } catch (_) {}
  }

  for (const name of ["fullscreenElement", "webkitFullscreenElement", "webkitCurrentFullScreenElement"]) {
    try {
      Object.defineProperty(document, name, {
        configurable: true,
        get: () => activeElement,
      });
    } catch (_) {}
  }

  const installStyle = () => {
    if (document.getElementById("croni-pseudo-fullscreen-style")) return;
    const style = document.createElement("style");
    style.id = "croni-pseudo-fullscreen-style";
    style.textContent = `
      html.${rootClass},
      html.${rootClass} body {
        overflow: hidden !important;
      }

      [${marker}] {
        position: fixed !important;
        top: 2px !important;
        right: 2px !important;
        bottom: 2px !important;
        left: 2px !important;
        width: calc(100vw - 4px) !important;
        height: calc(100vh - 4px) !important;
        max-width: none !important;
        max-height: none !important;
        margin: 0 !important;
        overflow: hidden !important;
        transform: none !important;
        z-index: 2147483647 !important;
        background: #000 !important;
      }

      video[${marker}] {
        inset: auto !important;
        width: calc(100vw - 4px) !important;
        height: calc(100vh - 4px) !important;
        object-fit: contain !important;
      }

      [${marker}] video {
        max-width: 100% !important;
        max-height: 100% !important;
        object-fit: contain !important;
      }
    `;
    (document.head || document.documentElement).appendChild(style);
  };

  if (document.documentElement) installStyle();
  else document.addEventListener("DOMContentLoaded", installStyle, { once: true });

  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && activeElement) {
      event.preventDefault();
      event.stopImmediatePropagation();
      void leave();
    }
  }, true);
})();
