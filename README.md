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
cargo run -p kitsune-compositor-cli -- appearance capabilities --contract-v1
cargo run -p kitsune-compositor-cli -- appearance preview --image /ruta/wallpaper.png --contract-v1
cargo run -p kitsune-compositor-cli -- appearance apply --image /ruta/wallpaper.png --dry-run --contract-v1
cargo run -p kitsune-compositor-cli -- active-media list --contract-v1
cargo run -p kitsune-compositor-cli -- appearance apply --output DP-1 --dry-run --contract-v1
cargo run -p kitsune-compositor-cli -- appearance policy show --contract-v1
cargo run -p kitsune-compositor-cli -- display-manager capabilities --contract-v1
cargo run -p kitsune-compositor-cli -- sddm status --contract-v1
cargo run -p kitsune-compositor-cli -- sddm theme plan --descriptor /ruta/absoluta/config.json --contract-v1
```

El dominio SDDM se implementa únicamente en este compositor refactorizado.
Valida descriptores, materializa medios, instala solo el tema administrado,
activa `/etc/sddm.conf.d/90-kisddm.conf` y conserva un backup simétrico. Las
operaciones mutables requieren `--confirm`; KiSDDM usa `--authorize` para elevar
exclusivamente esta llamada mediante Polkit.

El workspace produce el binario `kitsune-compositor`. El crate `backend` se puede probar con runners falsos y no necesita una sesion grafica real para validar normalizacion y seleccion de adapters.

## CI y releases

`.github/workflows/ci.yml` ejecuta formato, Clippy, pruebas, build release y una
comprobacion del contrato CLI en cada push o pull request hacia `main`.

Un tag SemVer que coincida con la version de `Cargo.toml` inicia
`.github/workflows/release.yml`. El workflow publica:

```text
kitsune-compositor-<version>-x86_64-unknown-linux-gnu.tar.zst
kitsune-compositor-<version>-x86_64-unknown-linux-gnu.manifest.json
kitsune-compositor-<version>-x86_64-unknown-linux-gnu.spdx.json
SHA256SUMS
```

El release incluye checksums y atestaciones de procedencia/SBOM. Crear el tag es
una operacion explicita; los pushes normales nunca publican un release.

No crear un tag hasta que CI haya aprobado el commit que se desea publicar:

```bash
git tag -a v0.1.0 -m "Kitsune Compositor v0.1.0"
git push origin v0.1.0
```

## Estado de migracion

Actualizacion 2026-09-24: Niri interpreta modos indexados, frecuencia en milihertz,
outputs desactivados y consulta el foco mediante `focused-output`. Relaciona
ventanas con outputs mediante workspaces y tolera ventanas sin `app_id`.
Apariencia detecta DMS por IPC y anuncia por ahora solo lectura/preview;
aplicacion y restauracion DMS permanecen pendientes. Estas comprobaciones de
lectura no certifican todavia la integracion completa del entorno.

El contrato read-only y los adapters Hyprland/Niri ya estan separados en el nuevo workspace. `watch outputs` y `watch focus` ofrecen JSON Lines mediante snapshots portables. `wallpaper runtime` aplica y controla `awww/swww` mediante argumentos exactos y publica automaticamente el wallpaper estatico activo por output. Kilivepaper publica el video y thumbnail representativo mediante el mismo registro `active-media`. `appearance apply --output` resuelve esa fuente sin leer estados privados de los productos. `appearance preview` extrae en Rust una paleta normalizada y cacheada sin depender de Node, ImageMagick ni el antiguo watcher de Kitsune. Detecta Caelestia, GNOME, KDE, Hyprland y XDG Portal. El proveedor Caelestia admite aplicacion y restauracion opt-in con `--confirm`, estado XDG, rollback y proteccion frente a cambios posteriores del usuario; los demas proveedores mutables siguen pendientes. El contrato de servicios materializa unidades `systemd --user` desde descriptores tipados y mantiene un registro propio con eliminacion simetrica. Los descriptores de automatizacion admiten un mapa `environment` portable que el adapter traduce de forma segura al gestor de servicios. `automation plan-batch/apply-batch` valida y materializa multiples intenciones con rollback conjunto; la activacion permanece explicita. Las capacidades se anuncian dinamicamente segun los adapters disponibles.
