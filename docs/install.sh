#!/bin/sh
# Instalador de rdd para Linux y macOS.
#
#   curl -fsSL https://jhonwick20262027-boop.github.io/rukito/install.sh | sh
#
# Variables: RDD_VERSION (p. ej. v0.1.0), RDD_INSTALL_DIR (por defecto
# ~/.local/bin). No usa sudo ni pide permisos de administrador.
set -eu

REPO="jhonwick20262027-boop/rukito"
INSTALL_DIR="${RDD_INSTALL_DIR:-$HOME/.local/bin}"
REQUESTED_VERSION="${RDD_VERSION:-}"

err() {
  printf 'rdd: %s\n' "$*" >&2
  exit 1
}

command -v curl >/dev/null 2>&1 || err "falta curl"
command -v tar >/dev/null 2>&1 || err "falta tar"

case "$(uname -s)" in
  Linux) os="unknown-linux-gnu" ;;
  Darwin) os="apple-darwin" ;;
  *) err "sistema no soportado: $(uname -s); usa 'cargo install rdd-cli'" ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
  *) err "arquitectura no soportada: $(uname -m); usa 'cargo install rdd-cli'" ;;
esac

target="${arch}-${os}"

if [ -z "$REQUESTED_VERSION" ]; then
  REQUESTED_VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
  [ -n "$REQUESTED_VERSION" ] || err "no se pudo resolver la última versión"
fi

version="${REQUESTED_VERSION#v}"
asset="rdd-${version}-${target}.tar.gz"
base="https://github.com/$REPO/releases/download/$REQUESTED_VERSION"

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rdd.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM

printf 'Descargando %s...\n' "$asset"
curl -fsSL -o "$tmp/$asset" "$base/$asset" \
  || err "no se pudo descargar $asset (¿existe la release $REQUESTED_VERSION?)"
curl -fsSL -o "$tmp/SHA256SUMS.txt" "$base/SHA256SUMS.txt" \
  || err "no se pudo descargar SHA256SUMS.txt"

expected=$(awk -v name="$asset" '{ f = $2; sub(/^\*/, "", f); if (f == name) { print $1; exit } }' "$tmp/SHA256SUMS.txt")
[ -n "$expected" ] || err "el checksum de $asset no está en SHA256SUMS.txt"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$asset" | awk '{ print $1 }')
else
  actual=$(shasum -a 256 "$tmp/$asset" | awk '{ print $1 }')
fi
[ "$actual" = "$expected" ] || err "checksum incorrecto para $asset"

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$INSTALL_DIR"
cp "$tmp/rdd-${version}-${target}/rdd" "$INSTALL_DIR/rdd"
chmod 0755 "$INSTALL_DIR/rdd"

"$INSTALL_DIR/rdd" --version

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    printf '\n%s no está en PATH. Añade esta línea a tu perfil:\n' "$INSTALL_DIR"
    printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR"
    ;;
esac
