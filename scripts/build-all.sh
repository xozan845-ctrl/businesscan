#!/usr/bin/env bash
# Compila el binario para el host actual y, si los targets están instalados,
# para los demás objetivos habituales. No instala toolchains ni linkers.
set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p dist

host="$(rustc -vV | sed -n 's/^host: //p')"
targets=("$host")

# Añade targets instalados con `rustup target add`.
if command -v rustup >/dev/null 2>&1; then
    while read -r target; do
        [[ "$target" == "$host" ]] && continue
        targets+=("$target")
    done < <(rustup target list --installed)
fi

for target in "${targets[@]}"; do
    echo "==> Compilando para $target"
    if cargo build --release --target "$target"; then
        suffix="$target"
        case "$target" in
            *windows*) ext=".exe" ;;
            *) ext="" ;;
        esac
        cp "target/$target/release/businesscan$ext" "dist/businesscan-$suffix$ext"
    else
        echo "!! Falló $target (¿falta el linker/CC para ese target?)" >&2
    fi
done

echo
echo "Artefactos en dist/:"
ls -lh dist/
