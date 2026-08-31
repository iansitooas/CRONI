# Seguridad de CRONI

Si encuentras una vulnerabilidad, no publiques contraseñas, cookies ni datos privados en un issue. Contacta al responsable del repositorio de forma privada y adjunta únicamente los pasos mínimos para reproducirla.

Las actualizaciones de CRONI sólo se aceptan desde GitHub mediante HTTPS y se comparan con el SHA-256 publicado en la misma versión. Para distribución pública, además se recomienda firmar cada ejecutable con la misma identidad Authenticode y proteger el repositorio con autenticación de dos factores, revisión de cambios y versiones inmutables.

CRONI utiliza el WebView2 instalado en Windows. Los usuarios deben mantener Windows y WebView2 actualizados, ejecutar CRONI como usuario normal y comprobar el dominio antes de introducir información personal.

Las listas del bloqueador se descargan por HTTPS desde EasyList y repositorios oficiales de Brave, se interpretan como datos mediante `adblock-rust` y nunca se ejecutan como código del sistema. El escudo permite desactivar el filtrado para un dominio si una regla rompe una página.
