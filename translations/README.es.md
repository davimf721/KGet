# ¡KelpsGet ahora es KGet! v1.5.0 (Nuevo Lanzamiento)

Un descargador moderno, ligero y versátil escrito en Rust para descargas rápidas y confiables a través de línea de comandos (CLI) e interfaz gráfica (GUI).

[English](README.md) | [Português](translations/README.pt-BR.md) | [Español](translations/README.es.md)

## Capturas de pantalla
- GUI:
 <img src="https://github.com/user-attachments/assets/30f77e72-aaac-454f-ace4-947b92411bf7"  width="600"/>
 
- Torrent en `localhost:9091/transmission/web/`:
 <img src="https://github.com/user-attachments/assets/d80b60d7-f53e-4198-8e11-1cacf0e78958"  width="600"/>

- CLI:
 <img src="https://github.com/user-attachments/assets/c2e512fe-be46-42b7-8763-fdc51a7233df"  width="600"/>

- Interactivo:
<img src="image.png"  width="600"/>

## Cómo funciona (Resumen)
1. **Barra de progreso (CLI):** Muestra velocidad, tiempo estimado y bytes transferidos.
2. **Nomenclatura inteligente de archivos:**
    * Usa el nombre del archivo de la URL.
    * Usa `index.html` por defecto si la URL termina con `/`.
3. **Manejo de errores:** Termina con código 1 en errores HTTP (ej: 404).
4. **Verificación de espacio:** Verifica el espacio disponible en disco.
5. **Reintento automático:** Reintenta la descarga en fallos de red.
6. **Modo de descarga avanzado (HTTP/HTTPS):** Descargas en chunks paralelos, soporta reanudación.
7. **Soporte de proxy:** HTTP, HTTPS, SOCKS5 con autenticación.
8. **Características de optimización:** Compresión (para caché), caché de archivos, límite de velocidad.
9. **Descargas de Torrent:** Añade enlaces magnet al `transmission-daemon` para descarga.
10. **Descargas FTP/SFTP:** Conecta a servidores FTP/SFTP para transferir archivos.

## Características

Vea la lista completa de características y cambios recientes en el [CHANGELOG](CHANGELOG.md).

## ¡KGet ahora es también un Crate!
Si desea usar KGet como un crate, haga clic [aquí](LIB.md).

## Instalación

### Opción 1: Compilar desde la fuente (Recomendado para obtener todas las características)

Necesitará tener Rust instalado. Si no lo tiene, instálelo en [rustup.rs](https://rustup.rs/).

Instale algunas dependencias:
Para sistemas basados en Debian/Ubuntu:
```bash
sudo apt update
sudo apt install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev pkg-config
```
Para Fedora:
```bash
sudo dnf install -y libxcb-devel libxkbcommon-devel openssl-devel pkg-config
```

Clone el repositorio y compile el proyecto:
```bash
git clone https://github.com/davimf721/KGet.git
cd KGet
cargo build --release
```
El ejecutable estará en `target/release/kget`. Puede copiarlo a un directorio en su `PATH`:
```bash
sudo cp target/release/kget /usr/local/bin/
```

### Opción 2: Instalar vía Cargo
```bash
cargo install kelpsget
```
Si encuentra problemas con la GUI al instalar vía `cargo install`, compilar desde la fuente es más confiable.

### Opción 3: Descargar binarios precompilados
Verifique la sección [Releases](https://github.com/davimf721/KGet/releases) para los binarios más recientes para su SO.

#### Linux/macOS:
```bash
chmod +x kelpsget  # Hacer ejecutable
./kelpsget [URL]   # Ejecutar directamente
```
#### Windows:
Ejecute el archivo `.exe` directamente.

### Requisito adicional para descargas de Torrent: Transmission Daemon

KGet usa `transmission-daemon` para gestionar descargas de torrent.

**1. Instalar Transmission Daemon:**
* **Debian/Ubuntu:**
     ```bash
     sudo apt update
     sudo apt install transmission-daemon
     ```
* **Fedora:**
     ```bash
     sudo dnf install transmission-daemon
     ```
* **Arch Linux:**
     ```bash
     sudo pacman -S transmission-cli
     ```

**2. Detener el Daemon para configuración:**
```bash
sudo systemctl stop transmission-daemon
```

**3. Configurar Transmission:**
Edite el archivo `settings.json`. Ubicaciones comunes:
* `/var/lib/transmission-daemon/info/settings.json` (Debian/Ubuntu, si se instala como servicio)
* `/var/lib/transmission/.config/transmission-daemon/settings.json` (Otra ruta común, verifique su sistema)
* `~/.config/transmission-daemon/settings.json` (si se ejecuta como usuario)

Use `sudo nano /var/lib/transmission-daemon/info/settings.json` (o la ruta correcta para su sistema).

Encuentre y modifique estas líneas:
```json
{
     // ...
     "rpc-authentication-required": true,
     "rpc-enabled": true,
     "rpc-password": "transmission", // Este es el valor que KGet usa por defecto para conectar a Transmission (recomendado)
     "rpc-port": 9091,
     "rpc-username": "transmission", // Nombre de usuario que KGet usa para conectar a Transmission
     "rpc-whitelist-enabled": false, // Para acceso local. Para acceso remoto, configure IPs.
     "download-dir": "/var/lib/transmission-daemon/downloads", // Directorio por defecto de descarga de Transmission
     // ...
}
```
**Importante:** Después de guardar e iniciar el `transmission-daemon`, reemplazará la contraseña en texto plano `rpc-password` por una versión con hash.

**4. (Opcional) Ajustar permisos del usuario del Daemon:**
Si `transmission-daemon` se ejecuta como un usuario específico (ej: `debian-transmission` o `transmission`), asegúrese que este usuario tiene permisos de escritura en los directorios de descarga que pretende usar con KelpsGet o el propio Transmission. Puede añadir su usuario al grupo del Transmission daemon:
```bash
sudo usermod -a -G debian-transmission su_usuario_linux # Para Debian/Ubuntu
# Verifique el nombre del grupo/usuario de Transmission en su sistema
```

**5. Iniciar Transmission Daemon:**
```bash
sudo systemctl start transmission-daemon
# Verificar estado:
sudo systemctl status transmission-daemon
```
Acceda a `http://localhost:9091` en su navegador. Deberá ver la interfaz web de Transmission y se le solicitará iniciar sesión con el `rpc-username` y `rpc-password` que configuró.

## Uso

### Línea de comandos (CLI)
```bash
kelpsget [OPCIONES] <URL>
```
**Ejemplos:**
* **Descarga HTTP/HTTPS:**
     ```bash
     kelpsget https://example.com/archivo.txt
     ```
* **Renombrar archivo de salida:**
     ```bash
     kelpsget -O nuevo_nombre.txt https://example.com/archivo.txt
     kelpsget -O ~/Downloads/ https://example.com/video.mp4 # Guarda como ~/Downloads/video.mp4
     ```
* **Descarga FTP:**
     ```bash
     kelpsget ftp://usuario:contraseña@ftp.example.com/archivo.zip
     kelpsget --ftp ftp://ftp.example.com/pub/archivo.txt
     ```
* **Descarga SFTP:**
     (Requiere configuración de clave SSH o contraseña si el servidor lo permite)
     ```bash
     kelpsget sftp://usuario@sftp.example.com/ruta/archivo.dat
     kelpsget --sftp sftp://usuario@sftp.example.com/ruta/archivo.dat -O local.dat
     ```
* **Descarga de Torrent (Enlace Magnet):**
     (Requiere `transmission-daemon` configurado y en ejecución)
     ```bash
     kelpsget "magnet:?xt=urn:btih:TU_HASH_AQUI&dn=NombreTorrent"
     kelpsget --torrent "magnet:?xt=urn:btih:TU_HASH_AQUI" -O ~/MisTorrents/
     ```
     KelpsGet añadirá el torrent a Transmission e intentará abrir la interfaz web (`http://localhost:9091`) para gestión.

* **Modo silencioso:**
     ```bash
     kelpsget -q https://example.com/archivo.txt
     ```
* **Modo de descarga avanzado (HTTP/HTTPS):**
     ```bash
     kelpsget -a https://example.com/archivo_grande.zip
     ```
* **Usar proxy:**
     ```bash
     kelpsget -p http://proxy:8080 https://example.com/archivo.txt
     ```
* **Proxy con autenticación:**
     ```bash
     kelpsget -p http://proxy:8080 --proxy-user usuario --proxy-pass contraseña https://example.com/archivo.txt
     ```
* **Límite de velocidad:**
     ```bash
     kelpsget -l 1048576 https://example.com/archivo.txt  # Limita a 1MB/s
     ```
* **Deshabilitar compresión (específico de KelpsGet, no HTTP):**
     ```bash
     kelpsget --no-compress https://example.com/archivo.txt
     ```
* **Deshabilitar caché (específico de KelpsGet):**
     ```bash
     kelpsget --no-cache https://example.com/archivo.txt
     ```

### Interfaz gráfica (GUI)
Para iniciar la GUI:
```bash
kelpsget --gui
```
La GUI permite que introduzca la URL, ruta de salida e inicie descargas. El estado y progreso se muestran en la interfaz.

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
     "compression": true, // Compresión para caché de KelpsGet
     "compression_level": 6,
     "cache_enabled": true,
     "cache_dir": "~/.cache/kelpsget", // Expanda ~ manualmente o use ruta absoluta
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
     "key_path": null // Ruta para clave SSH privada, ej: "~/.ssh/id_rsa"
  }
}
```
**Nota sobre `cache_dir` y `key_path`:** Si usa `~`, asegúrese que su programa expanda correctamente la tilde al directorio home del usuario, o use rutas absolutas.

## 🔗 Enlaces importantes
- 📚 [Documentación](https://davimf721.github.io/KelpsGet/)
- 📦 [crates.io](https://crates.io/crates/kelpsget)
- 💻 [GitHub](https://github.com/davimf721/KelpsGet)
- 📝 [Changelog](CHANGELOG.md)

## Puede ver publicaciones sobre el proyecto en otras comunidades:
- [Dev.to](https://dev.to/davimf7221/kelpsget-v014-modern-download-manager-in-rust-4b9f)
- [r/rust](https://www.reddit.com/r/rust/comments/1kt69vh/after_5_months_of_development_i_finally_released/)
- [PitchHut](https://www.pitchhut.com/project/kelpsget)
- [Hacker News](https://hn.algolia.com/?query=Show%20HN%3A%20KelpsGet%20%E2%80%93%20Modern%20download%20manager%20built%20in%20Rust&type=story&dateRange=all&sort=byDate&storyText=false&prefix&page=0)

## Contribuyendo
¿Quiere contribuir? ¡Consulte nuestra [guía de contribución](CONTRIBUTING.md)!

¿Encontró un error o quiere añadir una característica? ¡Abra un issue o envíe un PR!

🚀 Haga descargas fácilmente con la velocidad y confiabilidad de Rust. 🚀

## Licencia
Este proyecto está licenciado bajo la Licencia MIT - vea el archivo [LICENSE](LICENSE) para detalles.

