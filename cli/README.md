# Compositor CLI

Aqui se trasladara el contrato publico para detectar backend, listar outputs, consultar capacidades y ejecutar operaciones portables de renderer y servicios.

Kitowall, Kitsune y Kilivepaper enviaran las mismas solicitudes y recibiran el mismo schema. El CLI no expone un ejecutor de comandos arbitrarios.

Binario objetivo: `kitsune-compositor`.

`--lc` es una opcion global de desarrollo. El compositor la acepta en cualquier
posicion para compartir el mismo contexto de ejecucion local con sus consumidores;
no cambia el schema del contrato ni introduce rutas de Gekko App.

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
