# Compositor Backend

Logica compartida de deteccion de escritorio y adapters nativos. Incluye outputs, foco, eventos, renderer de wallpaper y administracion portable de servicios desde descriptores validados.

Hyprland, `awww` y `systemd --user` formaran el primer perfil completo. Los demas adapters se incorporaran sin cambiar solicitudes ni respuestas publicas.

No ejecuta shell arbitrario ni instala paquetes. Toda ruta, unidad y operacion mutable debe estar validada y devolver un inventario de artefactos creados.

El primer contrato mutable implementado es `wallpaper runtime`. Usa `ProcessExecutor` para ejecutar binario y argumentos exactos, valida namespaces/outputs/rutas y selecciona `awww` antes de recurrir a `swww`.
