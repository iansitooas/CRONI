(() => {
  const install = () => {
    if (document.getElementById("croni-fullscreen-video-fix")) return;
    const style = document.createElement("style");
    style.id = "croni-fullscreen-video-fix";
    style.textContent = `
      video:fullscreen,
      :fullscreen video,
      video:-webkit-full-screen,
      :-webkit-full-screen video {
        position: absolute !important;
        top: 2px !important;
        left: 2px !important;
        width: calc(100% - 4px) !important;
        height: calc(100% - 4px) !important;
        max-width: calc(100% - 4px) !important;
        max-height: calc(100% - 4px) !important;
        object-fit: contain !important;
      }
    `;
    (document.head || document.documentElement).appendChild(style);
  };

  if (document.documentElement) install();
  else document.addEventListener("DOMContentLoaded", install, { once: true });
})();
