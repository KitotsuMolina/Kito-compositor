#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
BUMP=""
PUSH=false
DRY_RUN=false
SKIP_TESTS=false

usage() {
  cat <<'EOF'
Uso: scripts/release.sh (--patch|--path|--minor|--major) [--dry-run] [--push] [--skip-tests]

Actualiza la versión Cargo, ejecuta las pruebas, crea el commit y el tag vX.Y.Z.
Con --push envía la rama y el tag a origin mediante un push atómico.
--path se acepta como alias de --patch.
EOF
}

fail() { printf 'release: %s\n' "$*" >&2; exit 1; }
set_bump() { [[ -z "$BUMP" ]] || fail "usa un solo incremento"; BUMP="$1"; }

while (($#)); do
  case "$1" in
    --patch|--path) set_bump patch ;;
    --minor) set_bump minor ;;
    --major) set_bump major ;;
    --push) PUSH=true ;;
    --dry-run) DRY_RUN=true ;;
    --skip-tests) SKIP_TESTS=true ;;
    -h|--help) usage; exit 0 ;;
    *) fail "opción desconocida: $1" ;;
  esac
  shift
done
[[ -n "$BUMP" ]] || fail "elige --patch, --minor o --major"

current="$(sed -n 's/^version = "\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n1)"
[[ -n "$current" ]] || fail "no se pudo leer Cargo.toml"
IFS=. read -r major minor patch <<<"$current"
case "$BUMP" in
  patch) ((patch += 1)) ;;
  minor) ((minor += 1)); patch=0 ;;
  major) ((major += 1)); minor=0; patch=0 ;;
esac
next="$major.$minor.$patch"
tag="v$next"
printf '%s -> %s (%s)\n' "$current" "$next" "$tag"
$DRY_RUN && exit 0

[[ -z "$(git -C "$ROOT" status --porcelain)" ]] || fail "hay cambios sin commit"
branch="$(git -C "$ROOT" symbolic-ref --quiet --short HEAD)" || fail "detached HEAD"
! git -C "$ROOT" rev-parse --verify --quiet "refs/tags/$tag" >/dev/null || fail "el tag $tag ya existe"
$PUSH && git -C "$ROOT" remote get-url origin >/dev/null || ! $PUSH || fail "falta origin"

temporary="$ROOT/Cargo.toml.release.$$"
awk -v old="$current" -v new="$next" '
  !done && $0 == "version = \"" old "\"" { print "version = \"" new "\""; done=1; next }
  { print }
  END { if (!done) exit 42 }
' "$ROOT/Cargo.toml" >"$temporary" || { rm -f -- "$temporary"; fail "no se pudo actualizar Cargo.toml"; }
mv -- "$temporary" "$ROOT/Cargo.toml"

(cd -- "$ROOT"; cargo check --workspace; $SKIP_TESTS || cargo test --workspace --locked)
git -C "$ROOT" add Cargo.toml Cargo.lock
git -C "$ROOT" commit -m "chore(release): $tag"
git -C "$ROOT" tag -a "$tag" -m "$tag"
if $PUSH; then
  git -C "$ROOT" push --atomic origin "$branch" "refs/tags/$tag"
else
  printf 'Commit y tag locales creados. Usa --push para publicarlos.\n'
fi
