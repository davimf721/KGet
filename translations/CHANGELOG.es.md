# Registro de cambios

[English](../CHANGELOG.md) | [Português](CHANGELOG.pt-BR.md) | [Español](CHANGELOG.es.md)

Todos los cambios notables en este proyecto serán documentados en este archivo.

El formato está basado en [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
y este proyecto se adhiere al [Versionado Semántico](https://semver.org/spec/v2.0.0.html).

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
