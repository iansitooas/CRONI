# Publicar CRONI y distribuir actualizaciones

## Primera publicación

1. Crea un repositorio público vacío en GitHub y sube este proyecto completo.
2. Comprueba que la pestaña **Actions** esté habilitada.
3. Confirma que la versión de `Cargo.toml` sea la que quieres publicar, por ejemplo `0.4.8`.
4. Crea y sube una etiqueta con la misma versión:

   ```powershell
   git tag v0.4.8
   git push origin v0.4.8
   ```

El flujo `.github/workflows/release.yml` ejecuta las pruebas, compila en Windows y crea una versión de GitHub con:

- `CRONI-PORTABLE-DESCARGA.zip`, la descarga recomendada para extraer y abrir; también incluye el script opcional de WebView2;
- `CRONI.exe`, usado también por el actualizador;
- `CRONI.exe.sha256`, usado para verificar que la descarga no fue modificada.

El repositorio `propietario/nombre` queda incorporado automáticamente al ejecutable construido por GitHub. Esa compilación busca una versión nueva al iniciar. Si existe, la descarga en segundo plano; aparece una flecha verde y el usuario puede pulsar **Actualizar y reiniciar**.

## Publicar las versiones siguientes

1. Cambia `version` en `Cargo.toml`, por ejemplo a `0.4.9`.
2. Ejecuta `cargo check` y `cargo test`.
3. Sube los cambios a GitHub.
4. Crea y sube `v0.4.9`.

No reemplaces archivos de una versión ya publicada. Crea siempre una etiqueta y una versión nuevas.

## Firma para Windows y SmartScreen

El flujo puede firmar el ejecutable si configuras estos secretos en **Settings > Secrets and variables > Actions**:

- `WINDOWS_CERTIFICATE_BASE64`: contenido Base64 de un certificado Authenticode `.pfx` emitido por una entidad pública de confianza;
- `WINDOWS_CERTIFICATE_PASSWORD`: contraseña del `.pfx`.

Un certificado autofirmado no elimina la advertencia. Otra opción recomendada por Microsoft es Artifact Signing; la opción que evita de forma más fiable el aviso de descarga es publicar mediante Microsoft Store. Nunca subas un `.pfx` ni su contraseña al repositorio.
