# Compositor CLI

Aqui se trasladara el contrato publico para detectar backend, listar outputs, consultar capacidades y ejecutar operaciones portables de renderer y servicios.

Kitowall, Kitsune y Kilivepaper enviaran las mismas solicitudes y recibiran el mismo schema. El CLI no expone un ejecutor de comandos arbitrarios.

Binario objetivo: `kitsune-compositor`.

`--lc` es una opcion global de desarrollo. El compositor la acepta en cualquier
posicion para compartir el mismo contexto de ejecucion local con sus consumidores;
no cambia el schema del contrato ni introduce rutas de Gekko App.

## Apariencia y paletas

```bash
kitsune-compositor appearance capabilities --contract-v1
kitsune-compositor appearance preview \
  --image /ruta/absoluta/wallpaper.png \
  --contract-v1
kitsune-compositor appearance current --contract-v1
kitsune-compositor appearance apply \
  --image /ruta/absoluta/wallpaper.png \
  --dry-run \
  --contract-v1
kitsune-compositor appearance apply \
  --image /ruta/absoluta/wallpaper.png \
  --confirm \
  --contract-v1
kitsune-compositor appearance restore --dry-run --contract-v1
kitsune-compositor appearance restore --confirm --contract-v1
```

La fuente tambien puede resolverse por monitor:

```bash
kitsune-compositor active-media list --contract-v1
kitsune-compositor active-media get --output DP-1 --contract-v1
kitsune-compositor appearance preview --output DP-1 --contract-v1
kitsune-compositor appearance apply --output DP-1 --dry-run --contract-v1
kitsune-compositor appearance apply --output DP-1 --confirm --contract-v1
kitsune-compositor appearance policy show --contract-v1
kitsune-compositor appearance policy enable \
  --output DP-1 \
  --confirm \
  --contract-v1
kitsune-compositor appearance policy disable --contract-v1
```

Los productores publican mediante `active-media publish`; no escriben el JSON
directamente. El estado vive en
`$XDG_STATE_HOME/kitsune-compositor/active-media.json`. Para contenido estatico
`source` y `representative_image` son la misma imagen. Para livewallpapers,
`source` es el video y `representative_image` es un thumbnail o frame estable.
El monitor elige la fuente cromatica; Caelestia aplica la paleta globalmente a
la sesion.

La politica automatica es opt-in. `policy enable` guarda el monitor de
referencia y el consentimiento para que Caelestia pueda ejecutar su `postHook`
durante futuras rotaciones. Aplica inmediatamente la fuente actual y luego
sincroniza solo las publicaciones de ese output. `policy disable` detiene
cambios futuros, pero no restaura por si solo el tema anterior; para ello se
usa `appearance restore --confirm`.

La sincronizacion automatica se serializa entre procesos. Cada solicitante
adquiere el bloqueo, vuelve a leer `active-media` y aplica la fuente mas
reciente; si esa imagen ya coincide con el estado de apariencia, no vuelve a
ejecutar Caelestia. Esto evita carreras y propagaciones repetidas cuando una
rotacion, KiUI y un servicio publican cambios casi al mismo tiempo.

`preview` decodifica PNG, JPEG, WebP, GIF o TIFF en Rust, reduce la imagen,
construye un histograma y devuelve colores dominante, vibrante, atenuado,
acentos claro/medio/oscuro, primer plano con contraste y candidatos ponderados.
La cache XDG se identifica por ruta, tamano y fecha de modificacion. Si no se
puede escribir, la paleta se devuelve con `cache_warning`.

Kitowall debe proporcionar la imagen estatica aplicada. Kilivepaper debe
proporcionar un thumbnail o frame estable. Kitsune Spectro consume el contrato;
ninguno mantiene su propio extractor.

La deteccion prioriza el proveedor de paleta completa de Caelestia dentro de
Hyprland y ofrece fallbacks para GNOME, KDE, Hyprland y XDG Portal. Actualmente
solo Caelestia anuncia `apply_supported` y `restore_supported`.

`--dry-run` devuelve el plan exacto sin consultar ni modificar Caelestia.
La ejecucion real exige `--confirm` porque `caelestia wallpaper` puede ejecutar
el `postHook` configurado por el usuario. Antes de la primera aplicacion se
guardan esquema, flavour, modo, variante y wallpaper originales en
`$XDG_STATE_HOME/kitsune-compositor/appearance.json`. Las aplicaciones
posteriores conservan ese origen. `restore` se rechaza con `STATE_CONFLICT` si
el usuario cambio manualmente el esquema o wallpaper despues de la aplicacion.

## Eventos

```bash
kitsune-compositor watch outputs --json-lines
kitsune-compositor watch focus --json-lines
```

`--poll-ms` configura el intervalo entre 100 y 60000 ms. `--once` emite el snapshot inicial y termina, lo que permite pruebas y consultas puntuales sin dejar procesos activos.

## Wallpaper runtime

```bash
kitsune-compositor wallpaper status --namespace kitowall --contract-v1
kitsune-compositor wallpaper start --namespace kitowall --contract-v1
kitsune-compositor wallpaper apply \
  --namespace kitowall \
  --output DP-1 \
  --image /ruta/absoluta/wallpaper.png \
  --transition-type simple \
  --contract-v1
kitsune-compositor wallpaper stop --namespace kitowall --contract-v1
```

El compositor prefiere `awww` y conserva `swww` como compatibilidad. No instala ninguno: solo ejecuta el adapter ya instalado. `--namespace` es obligatorio y toda aplicacion valida primero el output y la ruta canonica de la imagen.

## Automatizacion

```bash
kitsune-compositor automation plan --descriptor /ruta/rotation.automation.json --contract-v1
kitsune-compositor automation apply --descriptor /ruta/rotation.automation.json --contract-v1
kitsune-compositor automation status --id kitowall-next --contract-v1
kitsune-compositor automation start|stop|restart --id kitowall-next --contract-v1
kitsune-compositor automation enable|disable --id kitowall-next --contract-v1
kitsune-compositor automation remove --id kitowall-next --contract-v1
```

Los consumidores describen comando, tipo, autostart y periodicidad. Solo el adapter decide nombres de unidades, targets y rutas. El control y retiro resuelven artefactos exclusivamente desde el registro privado; para tareas programadas se procesa el scheduler antes de la tarea cuando corresponde. Actualmente esta implementado `systemd-user`.

## Servicios fisicos

```bash
kitsune-compositor service plan --descriptor /ruta/kitowall-next.service.json --contract-v1
kitsune-compositor service apply --descriptor /ruta/kitowall-next.service.json --contract-v1
kitsune-compositor service enable --id kitowall-next --contract-v1
kitsune-compositor service status --id kitowall-next --contract-v1
kitsune-compositor service remove --id kitowall-next --contract-v1
```

Los descriptores fisicos se conservan como API interna y de compatibilidad. Los productos nuevos deben usar `automation`. El compositor mantiene su registro propio; GekkoApp no interviene en la creacion ni control de unidades.
