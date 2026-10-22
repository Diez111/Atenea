#!/bin/bash
# Atenea installer: native Linux EDA workbench (GPL-3.0-or-later)
# Usage: ./install.sh [--prefix ~/.local]
set -euo pipefail
PREFIX="${1:-$HOME/.local}"
BIN="$PREFIX/bin"
APP="$HOME/.local/share/applications"
echo "== Atenea install =="
echo "prefix: $PREFIX"
command -v cargo >/dev/null || { echo "need rust toolchain (rustup.rs)"; exit 1; }
command -v ngspice >/dev/null || echo "WARN: ngspice not found — batch sim disabled (pacman -S ngspice)"
cargo build --release -p atenea --manifest-path "$(dirname "$0")/Cargo.toml"
mkdir -p "$BIN" "$APP"
install -m755 "$(dirname "$0")/target/release/atenea" "$BIN/atenea"
sed "s|Exec=.*|Exec=$BIN/atenea|" "$(dirname "$0")/atenea.desktop" > "$APP/atenea.desktop" 2>/dev/null || true
echo "installed: $BIN/atenea"
"$BIN/atenea" --help 2>&1 | head -2 || true
echo OK
