# Uso de KGet como una Crate (Librería)

KGet es un motor de descarga de alto rendimiento para Rust. Proporciona características avanzadas como división en partes paralelas, E/S de flujo y protección de la salud del disco (Escritura con Búfer), todo envuelto en una API amigable para el desarrollador.

[English](../LIB.md) | [Português](LIB.pt-br.md) | [Español](LIB.es.md)

## Instalación

Añade KGet a tu `Cargo.toml`:

```toml
[dependencies]
kget = "1.5.2"
```

## Componentes Principales

La librería expone los siguientes bloques fundamentales:

- **`download`**: Función estándar para transferencias de flujo único (HTTP/HTTPS/FTP/SFTP).
- **`AdvancedDownloader`**: Una estructura para descargas paralelas multi-hilo con optimización automática de RAM/Disco.
- **`DownloadOptions`**: Controla el comportamiento de la librería (Modo silencioso, Ruta de salida, Verificación de ISO).
- **`create_progress_bar`**: Fábrica para crear barras de progreso con el estilo de KGet (verde, fluida, con ETA).
- **`verify_iso_integrity`**: Utilidad independiente para el cálculo de sumas de comprobación SHA256.
- **`Config` / `Optimizer`**: Gestión integral de la configuración.

## Guía Práctica (Cookbook)

La mejor manera de aprender es consultando nuestro [Ejemplo Completo](examples/lib_usage.rs).

### Ejemplo: Integración Personalizada

```rust
use kget::{download, DownloadOptions, Config, Optimizer};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::default();
    let options = DownloadOptions {
        output_path: Some("archivo_personalizado.zip".into()),
        verify_iso: false,
        quiet_mode: false,
    };

    // Llamada simple de una línea al motor
    download("https://example.com/file.zip", config.proxy, Optimizer::new(config.optimization), options)?;
    Ok(())
}
```

## Comportamiento: Librería vs CLI

Para asegurar que KGet funcione perfectamente como una librería, seguimos estas reglas:

1. **Sin Prompts de Stdin**: Las funciones de la librería **nunca** usan `stdin`. No pausarán su programa para hacer preguntas.
2. **Control Programático**: Use `DownloadOptions { verify_iso: true }` para forçar la verificación, o `false` para omitirla.
3. **Optimizado por Defecto**: Incluso cuando se usa como lib, KGet utiliza `BufWriter` de 2MB por hilo y flujo de 16KB para asegurar que el sistema anfitrión siga respondiendo y el uso de RAM sea bajo (~30MB).

## Uso Avanzado

Para escenarios complejos, como el cambio programático de proxy o la construcción de su propia GUI sobre el descargador, explore la estructura `AdvancedDownloader` y el módulo `Config` en el código fuente.

---
KGet está construido con ❤️ en Rust para velocidad y confiabilidad.