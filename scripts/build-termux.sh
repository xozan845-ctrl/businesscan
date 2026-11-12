#!/usr/bin/env bash
# Instalación nativa en Termux. Requiere: pkg install rust clang pkg-config
set -euo pipefail

cd "$(dirname "$0")/.."
cargo build --release
mkdir -p "${PREFIX:-/data/data/com.termux/files/usr}/bin"
install -m755 target/release/businesscan "${PREFIX:-/data/data/com.termux/files/usr}/bin/businesscan"
echo "businesscan instalado en ${PREFIX:-/data/data/com.termux/files/usr}/bin/businesscan"
businesscan --version
