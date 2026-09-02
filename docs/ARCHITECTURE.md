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

`config.json` contiene inicio, plantilla de búsqueda, tiempo de descarte, marcadores y URLs de sesión. Los datos de navegación pertenecen al directorio de usuario de WebView2. Las escrituras ocurren al cambiar sesión/configuración y al salir, no durante cada fotograma.

## Bloqueo de contenido

El manejador nativo `ICoreWebView2::WebResourceRequested` entrega URL, página de origen, método y tipo de recurso a `adblock-rust`. Una coincidencia devuelve HTTP 204 antes de que el recurso publicitario o rastreador se descargue. Es el mismo motor de filtros mantenido por Brave.

En Windows se reemplaza `requestFullscreen()` por un modo de ventana completa simulado dentro del documento. El elemento solicitado pasa a ocupar el viewport mediante una regla aislada, y un canal IPC limitado exclusivamente a un booleano indica a CRONI cuándo debe ocultar o restaurar la barra nativa. `F` y `Esc` siguen funcionando, pero WebView2 nunca activa su capa HTML5 de pantalla completa, que en algunos equipos deja de presentar imagen y permanece negra incluso al salir. La ventana principal tampoco cambia de modo. Cada cambio de tamaño notifica al compositor mediante `NotifyParentWindowPositionChanged`, y al restaurar la interfaz se fuerza una presentación nueva. Además se usa `--disable-gpu-compositing` para evitar otra falla conocida de DirectComposition. La decodificación de video por hardware continúa disponible, aunque la composición final por software puede consumir algo más de CPU.

Al arrancar, CRONI carga un motor serializado desde disco y, en segundo plano cada 48 horas, compila EasyList, EasyPrivacy y las reglas oficiales de Brave. Si todavía no hay caché o una descarga falla, un conjunto inicial compacto mantiene protección básica. La interfaz puede excluir únicamente el dominio actual. Una segunda capa inyecta las reglas cosméticas que el motor calcula para la página, retira contenedores publicitarios genéricos y pulsa botones oficiales para omitir anuncios cuando aparecen. No modifica la velocidad, el silencio ni la posición del video.

Esto aproxima el filtrado de red de Brave sin fingir equivalencia completa: no incluye todas sus protecciones de huellas, listas regionales, reglas de scriptlets ni su ciclo de pruebas de compatibilidad.

## Caché y procesos

No se fuerza recolección de basura: WebView2 no ofrece una API estable para ello y los flags internos de Chromium son frágiles. El perfil compartido deja que el motor use caché de disco; `MemoryUsageLevel::Low` y la destrucción del WebView son las palancas públicas y medibles. Tampoco se crea un proceso por pestaña desde Rust: el reparto de procesos queda en manos del runtime del sistema, que puede compartir renderer/GPU/network según su modelo de seguridad.

## Seguridad y límites

- Las extensiones están deshabilitadas y no hay plugins de aplicación.
- El contenido web sólo dispone de un canal IPC de una vía que acepta el booleano de entrada/salida del modo video; no expone archivos, descargas, pestañas, comandos del sistema ni otros datos nativos. La interfaz Win32 envía el resto de sus comandos directamente al bucle de eventos de Rust.
- Ventanas emergentes se convierten en pestañas y la ventana solicitada se deniega.
- WebView2 se actualiza con el sistema, evitando fijar un motor antiguo dentro de la aplicación.
- El actualizador consulta únicamente la versión estable más reciente del repositorio incorporado al compilar, exige HTTPS y verifica el SHA-256 antes de reemplazar el ejecutable mediante un proceso auxiliar.
- Un navegador de producción aún necesitaría UI completa de permisos y certificados, aislamiento por perfiles y pruebas continuas de phishing y seguridad.
