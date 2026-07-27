# Compositor

Infraestructura compartida que abstrae escritorios, outputs, renderers, servicios y operaciones dependientes de la sesion grafica.

- `cli/`: contrato terminal estable para consumidores y diagnostico.
- `backend/`: deteccion y adapters de Hyprland, GNOME, KDE, Niri, renderers y administradores de servicios.

No tiene frontend ni contiene decisiones de packs, espectro o biblioteca live. Ejecuta primitivas estandarizadas solicitadas por los productos y devuelve resultados con el mismo schema para todos los entornos.

Configuracion objetivo: `~/.config/kitsune-compositor/`.

## Desarrollo local

```bash
cargo check --workspace
cargo test --workspace
cargo run -p kitsune-compositor-cli -- outputs --contract-v1
cargo run -p kitsune-compositor-cli -- watch outputs --json-lines --once
```

El workspace produce el binario `kitsune-compositor`. El crate `backend` se puede probar con runners falsos y no necesita una sesion grafica real para validar normalizacion y seleccion de adapters.

## Estado de migracion

El contrato read-only y los adapters Hyprland/Niri ya estan separados en el nuevo workspace. `watch outputs` y `watch focus` ofrecen JSON Lines mediante snapshots portables. `wallpaper runtime` aplica y controla `awww/swww` mediante argumentos exactos. El contrato de servicios materializa unidades `systemd --user` desde descriptores tipados y mantiene un registro propio con eliminacion simetrica. `automation plan-batch/apply-batch` valida y materializa multiples intenciones con rollback conjunto; la activacion permanece explicita. Las capacidades se anuncian dinamicamente segun los adapters disponibles.
