# Verificación de pantalla completa (Windows)

CRONI usa `ContainsFullScreenElementChanged` de WebView2 y una ventana sin bordes en el monitor actual. Se conserva la API HTML nativa, incluyendo Escape.

Para ejecutar la prueba, abre una compilación de CRONI con un perfil temporal y `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9231`. Abre un video público de YouTube y ejecuta:

```powershell
node tests/native-fullscreen-check.mjs 9231
```

Requiere Node.js con `fetch` y `WebSocket` globales. La prueba verifica que un contenedor con `ad-showing` sobreviva al filtro, que haya fotogramas decodificados antes/durante/después de F y Escape, que se restaure el tamaño y que recargar conserve un reproductor funcional. Guarda capturas y resultados en `dist/fullscreen-verification`. No uses un perfil personal con depuración remota; cierra la instancia de prueba al terminar.

También verifica visualmente la ventana de Windows: una captura interna del motor por sí sola no descarta problemas de presentación en pantalla. Comprueba entrar/salir con F y Escape, el botón del reproductor, la recarga y cerrar/cambiar de pestaña tras salir.

Referencias:

- [Evento oficial de WebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2#add_containsfullscreenelementchanged).
- [Controlador de pantalla completa de Chromium](https://chromium.googlesource.com/experimental/chromium/src/+/HEAD/chrome/browser/ui/exclusive_access/fullscreen_controller.h).
- [Problema de presentación WebView2 en Windows 11](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5574).
