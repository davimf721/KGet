# Guía de Contribución para KelpsGet

[English](../CONTRIBUTING.md) | [Português](CONTRIBUTING.pt-BR.md) | [Español](CONTRIBUTING.es.md)

En primer lugar, ¡gracias por considerar contribuir a KelpsGet! Son personas como tú las que hacen que KelpsGet sea una herramienta increíble.

## Código de Conducta

Este proyecto y todos sus participantes se rigen por nuestro [Código de Conducta](../CODE_OF_CONDUCT.md). Al participar, se espera que cumplas con este código. Por favor, reporta cualquier comportamiento inaceptable a [davimf721@gmail.com](mailto:davimf721@gmail.com).

## ¿Cómo Puedo Contribuir?

### Reportando Bugs

Antes de crear reportes de bugs, por favor verifica los issues existentes, ya que podrías descubrir que no necesitas crear uno nuevo. Cuando estés creando un reporte de bug, por favor incluye tantos detalles como sea posible:

* Usa un título claro y descriptivo
* Describe los pasos exactos que reproducen el problema
* Proporciona ejemplos específicos para demostrar los pasos
* Describe el comportamiento observado después de seguir los pasos
* Explica qué comportamiento esperabas ver y por qué
* Incluye capturas de pantalla si es posible
* Incluye la versión de KelpsGet que estás usando
* Incluye tu sistema operativo y versión

### Sugiriendo Mejoras

Si tienes una sugerencia para el proyecto, ¡nos encantaría escucharla! Solo sigue estos pasos:

* Usa un título claro y descriptivo
* Proporciona una descripción paso a paso de la mejora sugerida
* Proporciona ejemplos específicos para demostrar los pasos
* Describe el comportamiento actual y explica qué comportamiento esperabas ver
* Explica por qué esta mejora sería útil para la mayoría de los usuarios de KelpsGet

### Pull Requests

* Completa la plantilla requerida
* No incluyas números de issues en el título del PR
* Incluye capturas de pantalla y GIFs animados en tu pull request cuando sea posible
* Sigue la guía de estilo de Rust
* Incluye pruebas bien estructuradas y documentadas
* Documenta el nuevo código
* Termina todos los archivos con una nueva línea

## Proceso de Desarrollo

1. Haz un fork del repositorio
2. Clona tu fork: `git clone https://github.com/tu-usuario/KelpsGet.git`
3. Crea tu rama de feature: `git checkout -b feature/mi-nueva-feature`
4. Realiza tus cambios
5. Ejecuta las pruebas: `cargo test`
6. Formatea tu código: `cargo fmt`
7. Verifica con clippy: `cargo clippy`
8. Haz commit de tus cambios: `git commit -am 'Añade alguna feature'`
9. Haz push a la rama: `git push origin feature/mi-nueva-feature`
10. Envía un pull request

## Guías de Estilo

### Mensajes de Commit de Git

* Usa el tiempo presente ("Añade feature" no "Añadida feature")
* Usa el modo imperativo ("Mover cursor a..." no "Mueve cursor a...")
* Limita la primera línea a 72 caracteres o menos
* Referencia issues y pull requests libremente después de la primera línea
* Considera comenzar el mensaje del commit con un emoji aplicable:
    * 🎨 `:art:` al mejorar el formato/estructura del código
    * 🐎 `:racehorse:` al mejorar el rendimiento
    * 🚱 `:non-potable_water:` al corregir memory leaks
    * 📝 `:memo:` al escribir documentación
    * 🐛 `:bug:` al corregir un bug
    * 🔥 `:fire:` al eliminar código o archivos
    * 💚 `:green_heart:` al corregir el build del CI
    * ✅ `:white_check_mark:` al añadir pruebas
    * 🔒 `:lock:` al tratar con seguridad
    * ⬆️ `:arrow_up:` al actualizar dependencias
    * ⬇️ `:arrow_down:` al hacer downgrade de dependencias

### Guía de Estilo de Rust

* Usa `cargo fmt` para formatear tu código
* Sigue las [Directrices de la API de Rust](https://rust-lang.github.io/api-guidelines/)
* Usa nombres de variables significativos
* Escribe documentación para APIs públicas
* Añade pruebas para nueva funcionalidad
* Mantén las funciones pequeñas y enfocadas
* Usa manejo de errores en lugar de pánicos
* Sigue las convenciones de nomenclatura de la biblioteca estándar

### Guía de Estilo de Documentación

* Usa [Markdown](https://daringfireball.net/projects/markdown/) para documentación
* Referencia funciones, clases y módulos en backticks
* Usa enlaces de sección al referirte a otras partes de la documentación
* Incluye ejemplos de código cuando sea posible
* Mantén la longitud de línea en un máximo de 80 caracteres
* Usa textos descriptivos para enlaces en lugar de "clic aquí"

## Notas Adicionales

### Etiquetas de Issues y Pull Requests

* `bug` - Algo no está funcionando
* `mejora` - Nueva feature o solicitud
* `documentación` - Mejoras o adiciones a la documentación
* `buen primer issue` - Bueno para principiantes
* `se necesita ayuda` - Se necesita atención extra
* `pregunta` - Se solicita más información
* `inválido` - Algo está mal
* `no se arreglará` - No se trabajará en esto

## Reconocimiento

Los contribuyentes que envíen un pull request válido serán añadidos a nuestro archivo [CONTRIBUTORS.md](../CONTRIBUTORS.md).

¡Gracias por contribuir a KelpsGet! 🚀 