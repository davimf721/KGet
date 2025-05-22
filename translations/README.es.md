# KelpsGet v0.1.4 (Nueva Version)

Un descargador moderno, ligero y versátil escrito en Rust para descargas de archivos rápidas y confiables a través de la línea de comandos (CLI) y la interfaz gráfica de usuario (GUI).

[English](README.md) | [Português](translations/README.pt-BR.md) | [Español](translations/README.es.md)

## Características
✅ Herramienta CLI y GUI sencilla para descargar archivos vía HTTP/HTTPS.<br>
✅ Soporte para descargas vía FTP y SFTP.<br>
✅ Soporte para descargas de Torrents (enlaces magnéticos) vía integración con Transmission.<br>
✅ Barra de progreso con velocidad en tiempo real y seguimiento de ETA (CLI).<br>
✅ Nombres de salida personalizados (bandera -O para renombrar archivos descargados).<br>
✅ Detección de tipo MIME y manejo adecuado de archivos.<br>
✅ Multiplataforma (Linux, macOS, Windows).<br>
✅ Modo silencioso para scripts.<br>
✅ Verificación automática de espacio antes de la descarga.<br>
✅ Reintento automático en caso de fallo de conexión.<br>
✅ Validación de nombre de archivo.<br>
✅ Visualización detallada de información de descarga.<br>
✅ Modo de descarga avanzado con fragmentos paralelos y capacidad de reanudación (HTTP/HTTPS).<br>
✅ Soporte para Proxy (HTTP, HTTPS, SOCKS5).<br>
✅ Compresión y caché automáticos (para optimizaciones específicas de KelpsGet).<br>
✅ Límite de velocidad y control de conexión.<br>

## Instalación

### Opción 1: Compilar desde el código fuente (Recomendado para obtener todas las características)

Necesitarás tener Rust instalado. Si no lo tienes, instálalo desde [rustup.rs](https://rustup.rs/).

Para compilar con todas las características, incluida la GUI, es posible que necesites algunas dependencias de desarrollo.
Para sistemas basados en Debian/Ubuntu:
```bash
sudo apt update
sudo apt install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev pkg-config
```
Para Fedora:
```bash
sudo dnf install -y libxcb-devel libxkbcommon-devel openssl-devel pkg-config
```

Clona el repositorio y compila el proyecto:
```bash
git clone https://github.com/davimf721/KelpsGet.git # Reemplaza con la URL correcta de tu repositorio
cd KelpsGet
cargo build --release
```
El ejecutable estará en `target/release/kelpsget`. Puedes copiarlo a un directorio en tu `PATH`:
```bash
sudo cp target/release/kelpsget /usr/local/bin/
```

### Opción 2: Instalar vía Cargo (Puede no incluir todas las dependencias de la GUI por defecto)
```bash
cargo install kelpsget
```
Si encuentras problemas con la GUI al instalar vía `cargo install`, compilar desde el código fuente es más confiable.

### Opción 3: Descargar Binarios Precompilados
Consulta la sección de [Lanzamientos (Releases)](https://github.com/davimf721/KelpsGet/releases) para los últimos binarios para tu SO.

#### Linux/macOS:
```bash
chmod +x kelpsget  # Hacer ejecutable
./kelpsget [URL]    # Ejecutar directamente
```
#### Windows:
Ejecuta el archivo `.exe` directamente.

### Requisito Adicional para Descargas de Torrent: Demonio de Transmission

KelpsGet usa el demonio `transmission-daemon` para gestionar las descargas de torrent.

**1. Instalar el Demonio de Transmission:**
*   **Debian/Ubuntu:**
    ```bash
    sudo apt update
    sudo apt install transmission-daemon
    ```
*   **Fedora:**
    ```bash
    sudo dnf install transmission-daemon
    ```
*   **Arch Linux:**
    ```bash
    sudo pacman -S transmission-cli
    ```

**2. Detener el Demonio para Configuración:**
```bash
sudo systemctl stop transmission-daemon
```

**3. Configurar Transmission:**
Edita el archivo `settings.json`. Ubicaciones comunes:
*   `/var/lib/transmission-daemon/info/settings.json` (Debian/Ubuntu, si se instaló como servicio)
*   `/var/lib/transmission/.config/transmission-daemon/settings.json` (Otra ruta común, verifica tu sistema)
*   `~/.config/transmission-daemon/settings.json` (si se ejecuta como usuario)

Usa `sudo nano /var/lib/transmission-daemon/info/settings.json` (o la ruta correcta para tu sistema).

Encuentra y modifica estas líneas:
```json
{
    // ...
    "rpc-authentication-required": true,
    "rpc-enabled": true,
    "rpc-password": "transmission", // Este es el valor que KelpsGet usa por defecto para conectarse a Transmission (recomendado)
    "rpc-port": 9091,
    "rpc-username": "transmission", // Nombre de usuario que KelpsGet usa para conectarse a Transmission
    "rpc-whitelist-enabled": false, // Para acceso local. Para acceso remoto, configura IPs.
    "download-dir": "/var/lib/transmission-daemon/downloads", // Directorio de descarga por defecto de Transmission
    // ...
}
```
**Importante:** Después de guardar e iniciar `transmission-daemon`, reemplazará la `rpc-password` en texto plano con una versión hasheada.

**4. (Opcional) Ajustar Permisos del Usuario del Demonio:**
Si `transmission-daemon` se ejecuta como un usuario específico (ej: `debian-transmission` o `transmission`), asegúrate de que este usuario tenga permisos de escritura en los directorios de descarga que pretendes usar con KelpsGet o Transmission mismo. Puedes añadir tu usuario al grupo del demonio de Transmission:
```bash
sudo usermod -a -G debian-transmission tu_usuario_linux # Para Debian/Ubuntu
# Verifica el nombre del grupo/usuario de Transmission en tu sistema
```

**5. Iniciar el Demonio de Transmission:**
```bash
sudo systemctl start transmission-daemon
# Verificar estado:
sudo systemctl status transmission-daemon
```
Accede a `http://localhost:9091` en tu navegador. Deberías ver la interfaz web de Transmission y se te pedirá iniciar sesión con el `rpc-username` y `rpc-password` que configuraste.

## Uso

### Línea de Comandos (CLI)
```bash
kelpsget [OPCIONES] <URL>
```
**Ejemplos:**
*   **Descarga HTTP/HTTPS:**
    ```bash
    kelpsget https://example.com/file.txt
    ```
*   **Renombrar Archivo de Salida:**
    ```bash
    kelpsget -O nuevo_nombre.txt https://example.com/file.txt
    kelpsget -O ~/MisDescargas/ https://example.com/video.mp4 # Guarda como ~/MisDescargas/video.mp4
    ```
*   **Descarga FTP:**
    ```bash
    kelpsget ftp://usuario:contraseña@ftp.example.com/archivo.zip
    kelpsget --ftp ftp://ftp.example.com/pub/archivo.txt
    ```
*   **Descarga SFTP:**
    (Requiere configuración de clave SSH o contraseña si el servidor lo permite)
    ```bash
    kelpsget sftp://usuario@sftp.example.com/ruta/archivo.dat
    kelpsget --sftp sftp://usuario@sftp.example.com/ruta/archivo.dat -O local.dat
    ```
*   **Descarga de Torrent (Enlace Magnético):**
    (Requiere `transmission-daemon` configurado y en ejecución)
    ```bash
    kelpsget "magnet:?xt=urn:btih:TU_HASH_AQUI&dn=NombreDelTorrent"
    kelpsget --torrent "magnet:?xt=urn:btih:TU_HASH_AQUI" -O ~/MisTorrents/
    ```
    KelpsGet añadirá el torrent a Transmission e intentará abrir la interfaz web (`http://localhost:9091`) para su gestión.

*   **Modo Silencioso:**
    ```bash
    kelpsget -q https://example.com/file.txt
    ```
*   **Modo de Descarga Avanzado (HTTP/HTTPS):**
    ```bash
    kelpsget -a https://example.com/archivo_grande.zip
    ```
*   **Usar Proxy:**
    ```bash
    kelpsget -p http://proxy:8080 https://example.com/file.txt
    ```
*   **Proxy con Autenticación:**
    ```bash
    kelpsget -p http://proxy:8080 --proxy-user usuario --proxy-pass contraseña https://example.com/file.txt
    ```
*   **Límite de Velocidad:**
    ```bash
    kelpsget -l 1048576 https://example.com/file.txt  # Límite a 1MB/s
    ```
*   **Deshabilitar Compresión (específica de KelpsGet, no HTTP):**
    ```bash
    kelpsget --no-compress https://example.com/file.txt
    ```
*   **Deshabilitar Caché (específica de KelpsGet):**
    ```bash
    kelpsget --no-cache https://example.com/file.txt
    ```

### Interfaz Gráfica de Usuario (GUI)
Para iniciar la GUI:
```bash
kelpsget --gui
```
La GUI te permite ingresar la URL, la ruta de salida e iniciar descargas. El estado y el progreso se muestran en la interfaz.

## Configuración de KelpsGet
KelpsGet usa un archivo de configuración en:
- Windows: `%APPDATA%\kelpsget\config.json`
- Linux/macOS: `~/.config/kelpsget/config.json`

**Ejemplo de `config.json` para KelpsGet:**
```json
{
  "proxy": {
    "enabled": false,
    "url": null,
    "username": null,
    "password": null,
    "proxy_type": "Http"
  },
  "optimization": {
    "compression": true, // Compresión para la caché de KelpsGet
    "compression_level": 6,
    "cache_enabled": true,
    "cache_dir": "~/.cache/kelpsget", // Expande ~ manualmente o usa ruta absoluta
    "speed_limit": null,
    "max_connections": 4
  },
  "torrent": {
    "enabled": true,
    "transmission_url": "http://localhost:9091/transmission/rpc",
    "username": "transmission", // Usuario configurado en settings.json de Transmission
    "password": "transmission", // Contraseña configurada en settings.json de Transmission
    "max_peers": 50,
    "max_seeds": 50,
    "port": null,
    "dht_enabled": true,
    "default_download_dir": null // Directorio por defecto para descargas de torrent vía KelpsGet
  },
  "ftp": {
    "default_port": 21,
    "passive_mode": true
  },
  "sftp": {
    "default_port": 22,
    "key_path": null // Ruta a la clave privada SSH, ej: "~/.ssh/id_rsa"
  }
}
```
**Nota sobre `cache_dir` y `key_path`:** Si usas `~`, asegúrate de que tu programa expanda correctamente la tilde al directorio home del usuario, o usa rutas absolutas.

## Cómo Funciona (Resumen)
1.  **Barra de Progreso (CLI):** Muestra velocidad, ETA y bytes transferidos.
2.  **Nomenclatura Inteligente de Archivos:**
    *   Usa el nombre de archivo de la URL.
    *   Por defecto `index.html` si la URL termina con `/`.
3.  **Manejo de Errores:** Sale con código 1 en errores HTTP (ej: 404).
4.  **Verificación de Espacio:** Verifica el espacio en disco disponible.
5.  **Reintento Automático:** Reintenta la descarga en caso de fallo de red.
6.  **Modo de Descarga Avanzado (HTTP/HTTPS):** Descarga en fragmentos paralelos, soporta reanudación.
7.  **Soporte para Proxy:** HTTP, HTTPS, SOCKS5 con autenticación.
8.  **Características de Optimización:** Compresión (para caché), caché de archivos, límite de velocidad.
9.  **Descargas de Torrent:** Añade enlaces magnéticos a `transmission-daemon` para descarga.
10. **Descargas FTP/SFTP:** Se conecta a servidores FTP/SFTP para transferir archivos.

## Características de Seguridad
- Verificación de Espacio: Asegura suficiente espacio en disco.
- Validación de Nombre de Archivo: Previene inyección de ruta.
- Manejo Seguro de URLs.
- Soporte Seguro para Proxy.

## Contribuyendo
¿Encontraste un error o quieres añadir una característica? ¡Abre un issue o envía un PR!

🚀 Descarga archivos sin esfuerzo con la velocidad y confiabilidad de Rust. 🚀

## 🔗 Enlaces Importantes
- 📚 [Documentación](https://davimf721.github.io/KelpsGet/) (Actualizar si es necesario)
- 📦 [crates.io](https://crates.io/crates/kelpsget)
- 💻 [GitHub](https://github.com/davimf721/KelpsGet)
- 📝 [Changelog](CHANGELOG.md)

## 🎯 Próximos Pasos (Ejemplo - ajusta a tu proyecto)
- [X] Soporte para descarga FTP/SFTP
- [X] Soporte para descarga de Torrent
- [X] Interfaz GUI de Escritorio
- [ ] Interfaz web para monitoreo de descargas
- [ ] Integración con servicios de almacenamiento en la nube
- [ ] Sistema de plugins personalizados
- [ ] Mejoras en la compresión adaptativa
- [ ] Optimización del sistema de caché
- [ ] Soporte para protocolos de proxy adicionales
- [ ] Documentación multilingüe (en progreso)

¿Quieres contribuir? ¡Consulta nuestra [guía de contribución](CONTRIBUTING.md)!

## Licencia
Este proyecto está licenciado bajo la Licencia MIT - consulta el archivo [LICENSE](LICENSE) para detalles.