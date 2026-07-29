# Compositor Backend

Logica compartida de deteccion de escritorio y adapters nativos. Incluye outputs, foco, eventos, renderer de wallpaper y administracion portable de servicios desde descriptores validados.

Hyprland, `awww` y `systemd --user` formaran el primer perfil completo. Los demas adapters se incorporaran sin cambiar solicitudes ni respuestas publicas.

No ejecuta shell arbitrario ni instala paquetes. Toda ruta, unidad y operacion mutable debe estar validada y devolver un inventario de artefactos creados.

El primer contrato mutable implementado es `wallpaper runtime`. Usa `ProcessExecutor` para ejecutar binario y argumentos exactos, valida namespaces/outputs/rutas y selecciona `awww` antes de recurrir a `swww`.

`appearance` contiene el extractor Rust y la cache de paletas que antes estaban
divididos entre `Kitsune/src/color_resolver.rs` y
`Kitsune/scripts/wallpaper-accent-watcher.sh`. Los providers de apariencia
pertenecen al compositor; los productos solo intercambian contratos
versionados. El proveedor mutable de Caelestia captura el esquema y wallpaper
originales, ejecuta argumentos tipados sin shell, conserva el origen entre
aplicaciones sucesivas y restaura solo cuando el estado actual sigue siendo
propiedad del compositor.

`active_media` mantiene el contenido activo por output bajo XDG. El registro
solo contiene rutas canonicas existentes y separa `source` de
`representative_image`: ambas coinciden para estaticos; los livewallpapers usan
el video como fuente y su thumbnail como imagen representativa. Un cambio de
propietario suspende el registro anterior, por lo que retirar Kilivepaper
recupera automaticamente el ultimo estatico de Kitowall.
