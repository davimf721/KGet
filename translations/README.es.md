# KelpsGet v0.1.3

Un clon moderno y ligero de wget escrito en Rust para descargas de archivos rápidas y confiables desde la línea de comandos.

[English](../README.md) | [Português](README.pt-BR.md) | [Español](README.es.md)

## Características
✅ Herramienta CLI simple para descargar archivos vía HTTP/HTTPS.<br>
✅ Barra de progreso con velocidad en tiempo real y tiempo estimado.<br>
✅ Nombres de salida personalizados (flag -O para renombrar archivos).<br>
✅ Detección de tipo MIME y manejo adecuado de archivos.<br>
✅ Multiplataforma (Linux, macOS, Windows).<br>
✅ Modo silencioso para scripts.<br>
✅ Verificación automática de espacio antes de la descarga.<br>
✅ Reintento automático en caso de fallo de conexión.<br>
✅ Validación de nombre de archivo.<br>
✅ Soporte para diferentes tipos MIME.<br>
✅ Visualización detallada de información de descarga.<br>
✅ Modo de descarga avanzado con chunks paralelos y capacidad de reanudación.<br>
✅ Soporte para proxy (HTTP, HTTPS, SOCKS5).<br>
✅ Compresión y caché automáticos.<br>
✅ Limitación de velocidad y control de conexión.<br>

## Instalación
### Opción 1: Instalación vía Cargo
```bash
cargo install kelpsget
```
### Opción 2: Descargar Binarios Pre-compilados
Descarga el último binario para tu sistema operativo en [Release](https://github.com/davimf721/KelpsGet/releases)

### Linux/macOS:
```bash
chmod +x kelpsget  # Hacer ejecutable
./kelpsget [URL]   # Ejecutar directamente
```
### Windows:
Ejecuta el archivo .exe directamente.

## Ejemplos de Uso
Descarga Básica:
```bash
kelpsget https://ejemplo.com/archivo.txt
```
Renombrar el Archivo de Salida:
```bash
kelpsget -O nuevo_nombre.txt https://ejemplo.com/archivo.txt
```
Modo Silencioso:
```bash
kelpsget -q https://ejemplo.com/archivo.txt
```
Modo de Descarga Avanzado (Paralelo y Reanudable):
```bash
kelpsget -a https://ejemplo.com/archivo_grande.zip
```
Usando Proxy:
```bash
kelpsget -p http://proxy:8080 https://ejemplo.com/archivo.txt
```
Con Autenticación de Proxy:
```bash
kelpsget -p http://proxy:8080 --proxy-user usuario --proxy-pass contraseña https://ejemplo.com/archivo.txt
```
Limitación de Velocidad:
```bash
kelpsget -l 1048576 https://ejemplo.com/archivo.txt  # Límite de 1MB/s
```
Deshabilitar Compresión:
```bash
kelpsget --no-compress https://ejemplo.com/archivo.txt
```
Deshabilitar Caché:
```bash
kelpsget --no-cache https://ejemplo.com/archivo.txt
```

## Cómo Funciona
1. Barra de Progreso: Muestra velocidad de descarga, tiempo estimado y bytes transferidos.
2. Nombrado Inteligente de Archivos:
  - Usa el nombre del archivo de la URL (ej: archivo.txt de https://ejemplo.com/archivo.txt).
  - Usa index.html por defecto si la URL termina con /.
3. Manejo de Errores: Sale con código 1 en errores HTTP (ej: 404).
4. Verificación de Espacio: Verifica espacio disponible en disco antes de la descarga.
5. Reintento Automático: Intenta nuevamente la descarga en caso de fallo de conexión.
6. Modo de Descarga Avanzado:
  - Descarga en chunks paralelos para mejor rendimiento
  - Soporta reanudación de descargas interrumpidas
  - Maneja archivos grandes de manera eficiente
7. Soporte para Proxy:
  - Soporte para proxy HTTP, HTTPS y SOCKS5
  - Autenticación de proxy
  - Configuración flexible de proxy
8. Características de Optimización:
  - Compresión automática (gzip, brotli, lz4)
  - Caché de archivos para descargas repetidas más rápidas
  - Limitación de velocidad
  - Control de conexión

## Configuración
KelpsGet usa un archivo de configuración ubicado en:
- Windows: `%APPDATA%\kelpsget\config.json`
- Linux/macOS: `~/.config/kelpsget/config.json`

Ejemplo de configuración:
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
    "compression": true,
    "compression_level": 6,
    "cache_enabled": true,
    "cache_dir": "~/.cache/kelpsget",
    "speed_limit": null,
    "max_connections": 4
  }
}
```

## Características de Seguridad
- Verificación de Espacio: Garantiza espacio suficiente en disco antes de la descarga.
- Validación de Nombre de Archivo: Previene inyección de ruta.
- Manejo de URL: Procesa URLs de manera segura.
- Reintento Automático: Intenta nuevamente en caso de fallo de red.
- Soporte Seguro para Proxy: Conexiones proxy cifradas.

## Contribuyendo
¿Encontraste un error o quieres agregar una funcionalidad? ¡Abre un issue o envía un PR!

🚀 Realiza descargas sin esfuerzo con la velocidad y confiabilidad de Rust. 🚀

## 🔗 Enlaces Importantes
- 📚 [Documentación](https://davimf721.github.io/KelpsGet/)
- 📦 [crates.io](https://crates.io/crates/kelpsget)
- 💻 [GitHub](https://github.com/davimf721/KelpsGet)
- 📝 [Changelog](CHANGELOG.md)

## 🎯 Próximos Pasos
Estamos trabajando en las siguientes mejoras:

- [ ] Soporte para descargas vía FTP/SFTP
- [ ] Interfaz web para monitoreo de descargas
- [ ] Integración con servicios de almacenamiento en la nube
- [ ] Sistema de plugins personalizados
- [ ] Soporte para descargas vía torrent
- [ ] Mejoras en la compresión adaptativa
- [ ] Optimización del sistema de caché
- [ ] Soporte para más protocolos de proxy
- [ ] Interfaz gráfica de escritorio (GUI)
- [ ] Documentación en múltiples idiomas

¿Quieres contribuir con alguna de estas funcionalidades? ¡Revisa nuestra [guía de contribución](CONTRIBUTING.md)!

## Licencia
Este proyecto está licenciado bajo la Licencia MIT - ver el archivo [LICENSE](LICENSE) para más detalles. 