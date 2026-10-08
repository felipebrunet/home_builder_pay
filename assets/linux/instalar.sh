#!/usr/bin/env bash
# Instala Konstruado para el usuario actual (sin root):
#   ./assets/linux/instalar.sh [ruta/al/binario]   (por defecto: konstruado-*-linux-x86_64 junto al script o target/release/konstruado)
set -euo pipefail
aqui="$(cd "$(dirname "$0")" && pwd)"
bin="${1:-}"
if [ -z "$bin" ]; then
  bin="$(ls "$aqui"/konstruado-*-linux-x86_64 2>/dev/null | tail -1 || true)"
  [ -n "$bin" ] || bin="$aqui/../../target/release/konstruado"
fi
[ -x "$bin" ] || { echo "No encuentro el binario: $bin" >&2; exit 1; }
install -Dm755 "$bin" "$HOME/.local/bin/konstruado"
install -Dm644 "$aqui/konstruado.png" "$HOME/.local/share/icons/hicolor/256x256/apps/konstruado.png"
install -Dm644 "$aqui/konstruado.desktop" "$HOME/.local/share/applications/konstruado.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$HOME/.local/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
echo "Listo: ~/.local/bin/konstruado (asegurate de tener ~/.local/bin en el PATH)."
