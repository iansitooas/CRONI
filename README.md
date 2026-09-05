# CRONI

CRONI es un navegador de escritorio enfocado en reducir RAM. Está escrito en Rust y no incluye Electron ni empaqueta Chromium: usa el motor WebView2 instalado en Windows.

## Descarga directa

**[⬇ Descargar CRONI.exe](https://github.com/iansitooas/CRONI/releases/latest/download/CRONI.exe)**

**[📦 Descargar versión portable](https://github.com/iansitooas/CRONI/releases/latest/download/CRONI-PORTABLE-DESCARGA.zip)**

En Windows 10/11 normalmente basta con descargar el ejecutable y abrirlo. La versión portable incluye también el instalador de recursos.

## Qué incluye

- Barra de dirección y búsqueda, atrás, adelante, recarga e inicio.
- Interfaz superior Win32 nativa en Windows: no usa un segundo WebView para dibujar pestañas, botones o menús.
- Pantalla completa nativa: el motor gestiona el reproductor y CRONI adapta la ventana al monitor; Escape restaura la interfaz sin recargar el video.
- Pestañas y marcadores persistentes.
- Barra de accesos rápidos persistentes: incluye YouTube inicialmente y permite guardar o eliminar enlaces con un clic.
- Registro por usuario como navegador disponible para HTTP/HTTPS y acceso directo a la pantalla oficial de Aplicaciones predeterminadas de Windows.
- Gestor de descargas con «Guardar como» para cualquier extensión, incluidos archivos generados con enlaces `download` de tipo `blob:`/`data:`. Permite elegir carpeta y nombre, confirmar reemplazos, ver progreso, cancelar y mostrar el archivo en su carpeta.
- Icono propio de CRONI en la ventana, la barra de tareas y el ejecutable de Windows.
- Actualización en segundo plano desde GitHub Releases, verificación SHA-256 y reemplazo del ejecutable al reiniciar.
- Restauración de la sesión anterior.
- Modo ultraligero predeterminado: al cambiar de pestaña, destruye inmediatamente el WebView anterior y conserva su URL para restaurarlo al volver.
- Modo configurable con pestañas inactivas en nivel de memoria `Low` de WebView2 y descarte tras 1, 5, 15, 30 o 60 minutos.
- Descarte inmediato de todas las pestañas inactivas si el sistema emite una alerta de memoria.
- Bloqueador nativo basado en `adblock-rust`, el motor que usa Brave, con EasyList, EasyPrivacy y reglas oficiales de Brave actualizadas en segundo plano.
- Contador de solicitudes bloqueadas, protección activable por sitio y limpieza cosmética ligera.
- Pausa de audio y video al navegar o dejar CRONI en segundo plano, además de un modo de movimiento reducido.
- Navegaciones limitadas a HTTP/HTTPS y aislamiento de páginas web respecto del canal nativo de la aplicación.
- Extensiones de WebView2 deshabilitadas explícitamente.
- Espera dirigida por eventos: la aplicación no mantiene un bucle de sondeo activo.

## Arquitectura

```text
Ventana nativa (winit)
├── Barra, pestañas y menús nativos Win32 (sin motor web)
└── WebView de contenido para la pestaña activa
    ├── activa: visible + MemoryUsageLevel::Normal
    └── inactiva: WebView destruido; sólo conserva URL/título
```

Todas las pestañas comparten un `WebContext`, por lo que cookies, inicios de sesión y caché en disco se reutilizan. En Windows, la interfaz se dibuja directamente con GDI y controles Win32; sólo el sitio abierto necesita WebView2. Consulta [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) para los detalles y límites deliberados.

## Instalación rápida en Windows

La opción más sencilla es descargar y abrir directamente [CRONI.exe](https://github.com/iansitooas/CRONI/releases/latest/download/CRONI.exe). Windows 10/11 normalmente ya incluye WebView2.

Si el ejecutable no abre, muestra una pantalla negra o tiene una versión antigua de WebView2, descarga el ZIP portable. Incluye `INSTALAR_RECURSOS.ps1`; abre PowerShell dentro de la carpeta descomprimida, usa el siguiente comando y acepta el permiso de administrador de Windows:

```powershell
powershell -ExecutionPolicy Bypass -File .\INSTALAR_RECURSOS.ps1 -RuntimeOnly
```

## Compilar en Windows

Requisitos:

1. Windows 10/11 con [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (normalmente ya está instalado).
2. [Rust estable con MSVC](https://www.rust-lang.org/tools/install).
3. Visual Studio Build Tools con la carga de trabajo **Desktop development with C++**.

Después de clonar el repositorio, este único comando instala WebView2, Rust y Visual Studio Build Tools con C++ mediante `winget`:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\setup-windows.ps1
```

Cierra y abre PowerShell para actualizar el `PATH`. Después, en este directorio:

```powershell
cargo test
cargo run --release
```

El ejecutable optimizado queda en `target\release\croni.exe`. Los datos nuevos del perfil y `config.json` se guardan en `%LOCALAPPDATA%\CRONI`; una instalación anterior conserva automáticamente su perfil de `%LOCALAPPDATA%\Navegadir` para no perder sesiones.

Para distribuirlo sin instalador, copia `CRONI.exe` y `packaging\LEEME.txt` en una carpeta y comprímela. El flujo de GitHub ya genera `CRONI-PORTABLE-DESCARGA.zip` automáticamente. Consulta [cómo publicar y actualizar](docs/PUBLICAR_EN_GITHUB.md).

## Estado de Linux y macOS

El núcleo evita APIs exclusivas de Windows salvo la sugerencia de memoria `Low`, que se activa por compilación condicional. Las dependencias de compilación para Debian/Ubuntu son:

```bash
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev
```

Esta entrega es **Windows-first**. Antes de ejecutarla en Linux hay que añadir la inicialización/bombeo del loop GTK; para Wayland, además, la creación de WebViews hijas requiere el contenedor GTK específico de `wry`. macOS puede usar WKWebView, pero no dispone de la sugerencia `MemoryUsageLevel::Low`; el descarte por destrucción sí es portable. Estos adaptadores de plataforma quedan separados del núcleo de pestañas para no fingir soporte que todavía no se ha probado.

## Configuración

El botón **⇩** abre las descargas. El escudo muestra el bloqueo por sitio y el número de solicitudes detenidas. El menú **☰** contiene marcadores, estado de actualización, modo ultraligero y movimiento reducido. `home_url` y `search_url` se pueden cambiar en `%LOCALAPPDATA%\CRONI\config.json` después del primer cierre. La plantilla de búsqueda debe contener `{query}`.

## Alcance realista

WebView2 ejecuta aplicaciones modernas como Gmail, YouTube y redes sociales con el mismo motor web que Edge, pero esto no convierte a CRONI en un reemplazo de seguridad completo para un navegador comercial. Algunos proveedores pueden bloquear OAuth en WebViews embebidos por política propia. El bloqueador usa el mismo motor de reglas que Brave e intercepta los subrecursos antes de red, pero CRONI no incorpora todas las protecciones, listas regionales, excepciones ni revisiones de compatibilidad de Brave. Si una página falla, el escudo permite desactivar el filtrado solamente en ese sitio.

## Actualizaciones y aviso de Windows

Las compilaciones hechas por `.github/workflows/release.yml` conocen automáticamente su repositorio de origen. Al iniciar, CRONI consulta la última versión estable, descarga `CRONI.exe`, verifica el SHA-256 y sólo entonces ofrece **Actualizar y reiniciar**. Una compilación local no consulta ningún repositorio hasta que se construye mediante ese flujo.

SmartScreen no se puede desactivar desde CRONI ni debe pedirse al usuario que desactive su protección. Para reducir el aviso, firma todas las versiones con un certificado Authenticode público y constante o publica en Microsoft Store. El flujo admite un certificado guardado como secretos de GitHub; nunca almacenes el `.pfx` en el código.
