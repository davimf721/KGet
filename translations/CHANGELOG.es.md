# Registro de cambios

[English](../CHANGELOG.md) | [Português](CHANGELOG.pt-BR.md) | [Español](CHANGELOG.es.md)

Todos los cambios notables en este proyecto serán documentados en este archivo.

El formato está basado en [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
y este proyecto se adhiere al [Versionado Semántico](https://semver.org/spec/v2.0.0.html).

## [1.6.2] - 2026-04-28

### Corregido
- **Las descargas SFTP eran completamente no funcionales.** La implementación anterior pasaba la cadena completa `sftp://…` directamente a `TcpStream::connect` y la usaba como ruta remota, provocando un fallo inmediato en cada llamada SFTP. El módulo fue reescrito por completo:
  - La URL se parsea correctamente para extraer `host`, `puerto`, `usuario` y `ruta remota`.
  - Autenticación en orden de prioridad: contraseña en la URL → agente SSH activo → archivos de clave predeterminados (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`, `~/.ssh/id_ecdsa`).
  - Archivo transmitido en chunks de 32 KB con barra de progreso en tiempo real.
  - Mensajes de error claros y accionables en cada punto de fallo.
- **El login anónimo FTP fallaba cuando la URL no contenía usuario.** `url.username()` de la crate `url` devuelve una cadena vacía `""` (no `None`) cuando la URL no tiene segmento de usuario. Pasar `""` a `ftp.login()` hacía que los servidores FTP anónimos rechazaran la conexión. El downloader ahora usa `"anonymous"` como fallback.

### Añadido
- **Modo interactivo completamente implementado.** Anteriormente `kget --interactive` abría un REPL que solo imprimía `"Would download: …"` sin realizar ninguna descarga. El modo ahora está completo:
  - Banner de entrada con arte ASCII en fuente de bloques Unicode.
  - Editor de línea `rustyline` con historial de comandos persistente.
  - `download [flags] <url>` — activa el downloader correcto según los flags:
    - Por defecto: HTTP/HTTPS simple con retry y barra de progreso.
    - `-a` / `--advanced` / `--turbo`: `AdvancedDownloader` (paralelo con byte range, reanudable).
    - `--ftp`: downloader FTP con fallback anónimo.
    - `--sftp`: downloader SFTP con autenticación SSH multi-método.
    - `--torrent` o prefijo `magnet:?` detectado automáticamente: motor de torrent nativo.
    - Flags `-o <ruta>`, `-q` (silencioso), `--sha256 <hash>` soportados.
  - `config [show | set <clave> <valor>]`: lee y persiste configuraciones (`connections`, `speed-limit`, `compression`, `cache`).
  - Comandos `clear`, `version`, `help` / `?`; `get` y `dl` como alias de `download`.
  - Los errores se imprimen y el REPL continúa — un error de descarga nunca cierra la sesión.

### Cambiado
- **Manejo de errores en locks Mutex en `AdvancedDownloader`:** todas las llamadas `.unwrap()` en `Mutex::lock()` reemplazadas por `.expect("…")` con mensajes descriptivos.
- **Limpieza de la API pública de `Optimizer`:** eliminados atributos `#[allow(dead_code)]` de los métodos públicos `compress`, `get_cached_file` y `cache_file`.

## [1.6.1] - 2026-04-27

### Añadido
- La app macOS ahora valida enlaces magnet antes de crear la tarjeta de descarga.
- Las descargas completadas incluyen acciones Abrir Archivo y Abrir Carpeta.
- Menú contextual en las tarjetas de macOS: Copiar URL, Abrir Carpeta, Reiniciar y Eliminar.
- Atajos en la app macOS: `Cmd+V`, `Cmd+L`, `Esc` y `Delete`.
- Verificación SHA256 esperada por CLI `--sha256 <hash>` y por biblioteca con `DownloadOptions::expected_sha256`.
- Helper público `verify_file_sha256` para usuarios de la biblioteca.
- Notificaciones nativas de finalización y error en la GUI Rust para Linux y Windows mediante `notify-rust`.

### Cambiado
- URLs o magnets duplicados ahora enfocan la tarjeta existente en macOS en vez de crear otra.
- Las descargas avanzadas respetan el límite de conexiones del optimizador y rechazan respuestas inválidas de byte range.
- Documentación de biblioteca actualizada en inglés, portugués y español para la API actual.

### Corregido
- Los enlaces magnet inválidos se rechazan antes de activar el backend torrent.
- Un mismatch de SHA256 ahora hace fallar la descarga en vez de solo imprimir el hash calculado.

## [1.6.0] - 2026-02-28

### Añadido
- **App Nativo macOS (SwiftUI):** Aplicación macOS nativa completamente rediseñada con integración profunda al sistema.
  - Manejadores de esquema de URL (`kget://`, `magnet:`)
  - Asociaciones de archivo (`.torrent`)
  - Integración con barra de menú con acciones rápidas
  - Soporte para el menú de Servicios de macOS
  - Notificaciones nativas
  - Instalador DMG arrastrar-y-soltar con guía visual (cajas, flecha, texto de instrucción)
- **GUI Multiplataforma Mejorada:** Gran renovación visual para la GUI basada en egui (Linux/Windows).
  - Lista de descargas con seguimiento de múltiples descargas simultáneas
  - Badge TURBO para modo de descargas paralelas
  - Badge ISO para archivos ISO con verificación automática de integridad
  - Barra de progreso multi-segmento mostrando conexiones paralelas (C1, C2, C3, C4)
  - Barra de progreso de verificación con tema púrpura y animación de escudo
  - Indicador de conexiones (⚡ 4x) para modo turbo
  - Visualización de velocidad y ETA en tiempo real
  - Estado vacío con iconos de protocolo
  - Entrada de URL en línea única con controles integrados
  - Diseño compacto con nombres de archivos y URLs truncados
  - Dimensionamiento y alineación adecuados de botones
- **Mejoras Visuales:**
  - Tema oscuro mejorado con mejor contraste
  - Efectos de brillo animados en las barras de progreso
  - Badges e iconos coloreados por estado
  - Tipografía y espaciado mejorados
  - Fondo del instalador DMG con tema oscuro, cajas redondeadas, flecha chevron y texto de instrucción

### Cambiado
- **Script de Build:** Ahora cierra automáticamente las instancias de KGet en ejecución antes de compilar
- **Script de Build:** Compila el bundle de la app en `/tmp` para evitar que los atributos extendidos de iCloud interfieran con la firma de código
- **Seguimiento de Progreso:** Eliminado el límite artificial del 99%, ahora muestra progreso preciso de 0-100%
- **Verificación SHA256:** Usa CommonCrypto nativo en macOS con progreso en tiempo real
- **Progreso de Descarga Avanzada:** Ahora usa reporte de progreso vía stdout en lugar de monitoreo de tamaño de archivo

### Corregido
- Barra de progreso atascada en 90% en modo de descarga avanzada
- Barra de progreso "temblando" (saltos erráticos) durante descargas avanzadas debido a conflicto entre monitoreo de tamaño de archivo y progreso vía stdout
- Progreso de verificación sin mostrar feedback hasta la finalización
- Firma de código fallando en macOS debido a que iCloud añade atributos extendidos (`com.apple.FinderInfo`, `com.apple.provenance`)
- Iconos del instalador DMG desalineados con las cajas de fondo

## [1.5.2] - 2025-12-19

### Añadido
- **Manejo Inteligente de ISO**: Detección automática de archivos `.iso` mediante URL y tipo MIME.
- **Prevención de Corrupción**: Los archivos ISO ahora omiten las capas de descompresión/optimización para garantizar la integridad binaria 1:1.
- **Verificación de Integridad**: Se agregó una verificación opcional de suma de comprobación SHA256 al final de las descargas de ISO.

### Corregido
- **Optimización de Memoria y Disco**: Se refactorizó `AdvancedDownloader` para usar escritas en stream con `BufWriter`, reduciendo drásticamente el uso de RAM y evitando problemas de 100% de tiempo activo del disco.
- **Confirmación de Verificación**: Se corrigió un error por el cual la verificación de integridad se ejecutaba automáticamente en modo avanzado; ahora el programa solicita confirmación al usuario correctamente.
- **UI/UX**: Se limpió la salida de la terminal durante las descargas paralelas para una experiencia de barra de progreso más fluida.
- Se corrigió el error del compilador Rust `E0382` con respecto a la propiedad (ownership) del tipo `Mime` en `download.rs`.
- Se mejoró la seguridad de escritura de chunks paralelos para archivos binarios pesados.

## [1.5.1] - 2025-12-18

### Añadido
- Feature opcional `gui` en Cargo para que las dependencias de la interfaz gráfica sean opcionales; compile con `--features gui` para habilitar el soporte de GUI.
- Funciones de conveniencia de alto nivel: `kget::download(...)` y `kget::advanced_download(...)` para facilitar el uso como biblioteca.
- `create_progress_bar_factory(...)` exportado para permitir que los consumidores creen barras de progreso `indicatif`.
- Ejemplo `examples/lib_usage.rs` demostrando el uso de la biblioteca.
- Instrucciones de desarrollo Docker e integración `docker-compose` para simplificar la compilación, pruebas y contribuciones.

### Cambiado
- Actualizado README y `LIB.md` con instrucciones de uso de la biblioteca y ejemplos.
- `CONTRIBUTING.md` y traducciones actualizadas con el flujo de trabajo para colaboradores a través de Docker.
- División del código GUI: se agregó el módulo `gui_types` para que las compilaciones CLI funcionen sin la feature de GUI.

### Corregido / Varios
- Pequeñas correcciones en la documentación y actualizaciones de traducción (PT-BR/ES).

## [1.5.0] - 2025-05-26

### Añadido
- Nuevo crate público de Rust: KGet ahora puede ser usado como una biblioteca en tus propios proyectos Rust, haz clic [aquí](LIB.es.md) para saber más.
- Interfaz gráfica mejorada: fuentes más grandes, mejor diseño y controles más intuitivos.
- Integración con el portapapeles para fácil pegado de URLs.
- Botones de descarga y cancelación ahora siempre visibles y funcionales en la interfaz gráfica.
- **Modo interactivo:** Ejecuta `kget --interactive` para una experiencia tipo REPL con comandos como `download <url> [output]`, `help` y `exit`.

### Cambiado
- Proyecto renombrado de "KelpsGet" a "KGet" para simplicidad y consistencia.
- Esquema de versionado actualizado de 0.1.4 a 1.5.0 para permitir actualizaciones menores más frecuentes y seguimiento de versiones más claro.
- Lista de características movida del README al CHANGELOG para mantenimiento más fácil y mantener el README conciso.

### Eliminado
- Sección de características redundantes o excesivamente detalladas del README (ahora ver el CHANGELOG para todas las características).

## [0.1.4] - 2025-05-22

### Añadido
- Interfaz Gráfica de Usuario (GUI) para descargas más fáciles.
- Soporte para descarga FTP.
- Soporte para descarga SFTP (autenticación por contraseña y clave).
- Soporte para descarga de torrent vía enlaces magnet (integración con el daemon Transmission).
- Instrucciones detalladas para configuración del daemon Transmission en el README.

### Cambiado
- Refinada determinación de la ruta de salida para alinear comportamiento con `wget`.
- Asegurado que `final_path` sea siempre absoluto para evitar errores "Archivo o directorio no encontrado" en CWD.
- Actualizado README en inglés, portugués y español para reflejar todas las nuevas características e instrucciones de configuración.

### Corregido
- Resuelto error "Archivo o directorio no encontrado" al descargar sin `-O` asegurando rutas absolutas.
- Corregido `validate_filename` para verificar solo el nombre base del archivo, no la ruta completa.
- Resueltos problemas potenciales con `map_err` en `main.rs` para descargas de torrent y HTTP.

## [0.1.3] - 2025-03-11

### Añadido
- Modo de descarga avanzado con chunks paralelos y capacidad de reanudación
- Soporte automático de compresión (gzip, brotli, lz4)
- Sistema de caché inteligente para descargas repetidas más rápidas
- Limitación de velocidad y control de conexión
- Soporte de documentación en múltiples idiomas

### Cambiado
- Mejorado manejo de errores y retroalimentación del usuario
- Mejorada barra de progreso con información más detallada
- Optimizado uso de memoria para descargas de archivos grandes
- Actualizado sistema de configuración de proxy

### Corregido
- Corregido problemas de autenticación de proxy
- Resuelto problemas de creación de directorio de caché
- Corregido manejo de nivel de compresión
- Corregido manejo de ruta de archivo en Windows

### Seguridad
- Añadido manejo seguro de conexión proxy
- Mejorada validación de URL
- Mejorado saneamiento de nombre de archivo
- Añadida verificación de espacio antes de las descargas

## [0.1.2] - 2025-03-10

### Añadido
- Soporte para proxy (HTTP, HTTPS, SOCKS5)
- Autenticación de proxy
- Nombrado personalizado de archivo de salida
- Detección de tipo MIME

### Cambiado
- Mejorado cálculo de velocidad de descarga
- Mejorada visualización de la barra de progreso
- Mejores mensajes de error
- Documentación actualizada

### Corregido
- Corregido problemas de timeout de conexión
- Resuelto problemas de permisos de archivo
- Corregido análisis de URL
- Corregida visualización de la barra de progreso en Windows

## [0.1.1] - 2025-03-09

### Añadido
- Modo silencioso para integración con scripts
- Barra de progreso básica
- Visualización del tamaño del archivo
- Seguimiento de velocidad de descarga

### Cambiado
- Mejorado manejo de errores
- Mejorada interfaz de línea de comandos
- Mejor manipulación de archivos
- Instrucciones de instalación actualizadas

### Corregido
- Corregido problemas de manipulación de ruta
- Resuelto problemas de permisos
- Corregida visualización de progreso
- Corregido comportamiento de sobrescritura de archivo

## [0.1.0] - 2025-03-08

### Añadido
- Lanzamiento inicial
- Funcionalidad básica de descarga de archivo
- Interfaz de línea de comandos
- Manejo básico de errores
- Soporte multiplataforma
