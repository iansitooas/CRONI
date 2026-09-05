# Arquitectura y decisiones

## Elección tecnológica

La aplicación usa Rust, `winit` para ventana/eventos y `wry` para alojar el WebView del sistema. Frente a Electron, el binario no lleva otra distribución de Chromium. En Windows `wry` usa WebView2; en macOS usa WKWebView y en Linux WebKitGTK.

En Windows, las tres filas de interfaz son una ventana hija Win32 dibujada con GDI, con un control `EDIT` nativo para la dirección y menús Win32 para ajustes y descargas. No se crea un entorno WebView2 adicional para la interfaz. En otras plataformas se conserva provisionalmente la interfaz HTML embebida mientras se desarrollan adaptadores nativos equivalentes.

## Ciclo de vida de una pestaña

Cada pestaña conserva siempre `id`, URL, título y última actividad. Su `WebView` es opcional:

1. Al activarla, se crea o muestra el WebView y en Windows se marca `MemoryUsageLevel::Normal`.
2. Con el modo ultraligero predeterminado, al pasarla a segundo plano Rust elimina inmediatamente el WebView. Se libera el DOM, heap JavaScript y recursos del documento.
3. Si el usuario desactiva ese modo, la pestaña se oculta, se marca `MemoryUsageLevel::Low` y se elimina cuando vence `discard_after_minutes`.
4. Al reactivarla, se crea otro WebView en el contexto compartido y se carga la última URL. Cookies y almacenamiento en disco sobreviven; el estado puramente en memoria de la página no.

El bucle de eventos duerme hasta el próximo vencimiento (`ControlFlow::WaitUntil`), por lo que la política no requiere un temporizador que despierte continuamente.

## Persistencia

En Windows cada descarga solicita su destino mediante `IFileSaveDialog`, con confirmación antes de reemplazar un archivo existente. `DownloadStarting` obtiene un deferral y envía únicamente un ID al bucle de eventos; los objetos COM permanecen en el hilo de interfaz. El diálogo se abre después de devolver el callback de WebView2. Aceptar aplica `ResultFilePath` y registra esa ruta en el historial antes de completar el deferral. Cancelar, un error o el cierre de la aplicación cancela la solicitud pendiente; el deferral se completa al liberar la solicitud. La compilación 0.5.5 no se probó en ejecución por solicitud del usuario.

`config.json` contiene inicio, plantilla de búsqueda, tiempo de descarte, marcadores y URLs de sesión. Los datos de navegación pertenecen al directorio de usuario de WebView2. Las escrituras ocurren al cambiar sesión/configuración y al salir, no durante cada fotograma.

## Bloqueo de contenido

El manejador nativo `ICoreWebView2::WebResourceRequested` entrega URL, página de origen, método y tipo de recurso a `adblock-rust`. Una coincidencia devuelve HTTP 204 antes de que el recurso publicitario o rastreador se descargue. Es el mismo motor de filtros mantenido por Brave.

En Windows se conserva la API HTML Fullscreen del motor. El evento oficial `ContainsFullScreenElementChanged` informa a Rust; el bucle de eventos oculta la barra nativa, activa `Fullscreen::Borderless(None)` en el monitor actual y ajusta los límites del WebView. Al salir, winit restaura la ventana y CRONI restaura la barra sin ocultar/recrear el controlador. También se restaura el host al cerrar la pestaña activa o navegar. No se sobrescriben `requestFullscreen` ni `document.fullscreenElement`, ni se depende de mensajes JavaScript. Es la separación entre pantalla completa del documento y de la ventana descrita por Chromium, adaptada a la [API de WebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2#add_containsfullscreenelementchanged).

Desde 0.5.6 se usa la aceleración gráfica predeterminada de WebView2, sin forzar `--disable-gpu-compositing` ni `--disable-direct-composition` para todas las tarjetas. El menú permite activar «Video compatible sin GPU» (`--disable-gpu`) para diagnosticar fallos de controladores gráficos; requiere cerrar todas las instancias y volver a abrir CRONI. La elección queda fija para todos los WebViews de esa ejecución. Este modo puede aumentar CPU y consumo energético. Sigue las [recomendaciones de Microsoft](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance#enable-hardware-acceleration); no garantiza compatibilidad con cualquier hardware.

La pérdida de foco espera 750 ms y comprueba la ventana principal en primer plano antes de pausar multimedia o reducir memoria. Así, mover foco al WebView o entrar/salir de pantalla completa no se interpreta inmediatamente como abandonar la aplicación.

Las descargas no filtran extensiones ni MIME: el selector muestra todos los archivos, conserva la extensión sugerida y no sigue accesos directos `.lnk`. Los enlaces explícitos `download` de tipo `blob:`/`data:` permanecen en su documento creador y llegan al gestor nativo sin copiar sus bytes a Rust. Estos esquemas se admiten dentro del renderer, no desde argumentos externos ni restauración de sesión. Las páginas que previsualizan un PDF o video siguen necesitando su acción Descargar/Guardar; no se convierten todas las navegaciones en descargas. La versión 0.5.6 se compila sin pruebas de ejecución por solicitud del usuario.

El filtro cosmético de respaldo usa selectores publicitarios específicos. No elimina clases genéricas `ad-*`: estados como `ad-showing` pertenecen al propio reproductor de YouTube y borrar ese nodo también elimina el video.

Al arrancar, CRONI carga un motor serializado desde disco y, en segundo plano cada 48 horas, compila EasyList, EasyPrivacy y las reglas oficiales de Brave. Si todavía no hay caché o una descarga falla, un conjunto inicial compacto mantiene protección básica. La interfaz puede excluir únicamente el dominio actual. Una segunda capa inyecta las reglas cosméticas que el motor calcula para la página, retira contenedores publicitarios genéricos y pulsa botones oficiales para omitir anuncios cuando aparecen. No modifica la velocidad, el silencio ni la posición del video.

Esto aproxima el filtrado de red de Brave sin fingir equivalencia completa: no incluye todas sus protecciones de huellas, listas regionales, reglas de scriptlets ni su ciclo de pruebas de compatibilidad.

## Caché y procesos

Desde 0.5.4 el filtro DOM analiza solamente los subárboles insertados y evita recorrer dos veces un descendiente incluido en el mismo lote. Se desconecta mientras el documento está oculto y recupera la limpieza al volver a ser visible. La barra nativa no clona listas ni repinta durante pantalla completa y su animación de carga se detiene mientras está oculta. Solo el WebView activo recibe cambios de tamaño; los conservados en segundo plano ajustan sus límites al activarse, evitando ampliar todas sus superficies al tamaño del monitor. Estas mejoras no imponen un límite de RAM ni cambian la calidad del video. El ahorro real depende del sitio y debe medirse; no se ejecutaron pruebas de esta versión por solicitud del usuario.

No se fuerza recolección de basura: WebView2 no ofrece una API estable para ello y los flags internos de Chromium son frágiles. El perfil compartido deja que el motor use caché de disco; `MemoryUsageLevel::Low` y la destrucción del WebView son las palancas públicas y medibles. Tampoco se crea un proceso por pestaña desde Rust: el reparto de procesos queda en manos del runtime del sistema, que puede compartir renderer/GPU/network según su modelo de seguridad.

## Seguridad y límites

- Las extensiones están deshabilitadas y no hay plugins de aplicación.
- Los mensajes web y objetos nativos accesibles desde páginas están deshabilitados. La pantalla completa procede de un evento del motor; la interfaz Win32 envía el resto de comandos directamente al bucle de Rust.
- Ventanas emergentes se convierten en pestañas y la ventana solicitada se deniega.
- WebView2 se actualiza con el sistema, evitando fijar un motor antiguo dentro de la aplicación.
- El actualizador consulta únicamente la versión estable más reciente del repositorio incorporado al compilar, exige HTTPS y verifica el SHA-256 antes de reemplazar el ejecutable mediante un proceso auxiliar.
- Un navegador de producción aún necesitaría UI completa de permisos y certificados, aislamiento por perfiles y pruebas continuas de phishing y seguridad.
