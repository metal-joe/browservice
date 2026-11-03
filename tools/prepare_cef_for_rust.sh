#!/bin/bash
# Converts an extracted CEF standard/minimal binary distribution (with separate
# Release/, Resources/ and include/ directories) into the flat layout expected by
# the cef-dll-sys crate (all runtime files in one directory plus include/ and
# archive.json). Files are symlinked, not copied.
#
# Usage: tools/prepare_cef_for_rust.sh <extracted CEF distribution> <output dir>
# Then build with: CEF_PATH=<output dir> cargo build --release
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <extracted CEF distribution> <output dir>" >&2
    exit 1
fi

SRC="$(realpath "$1")"
OUT="$2"

if [ ! -f "$SRC/include/cef_version.h" ] || [ ! -d "$SRC/Release" ] || [ ! -d "$SRC/Resources" ]; then
    echo "ERROR: $SRC does not look like an extracted CEF binary distribution" >&2
    exit 1
fi

VERSION="$(sed -n 's/^#define CEF_VERSION "\(.*\)"$/\1/p' "$SRC/include/cef_version.h")"
if [ -z "$VERSION" ]; then
    echo "ERROR: Could not read CEF_VERSION from $SRC/include/cef_version.h" >&2
    exit 1
fi

mkdir -p "$OUT"
OUT="$(realpath "$OUT")"

for f in "$SRC"/Release/* "$SRC"/Resources/*; do
    ln -sfn "$f" "$OUT/$(basename "$f")"
done
ln -sfn "$SRC/include" "$OUT/include"
[ -d "$SRC/cmake" ] && ln -sfn "$SRC/cmake" "$OUT/cmake"
[ -d "$SRC/libcef_dll" ] && ln -sfn "$SRC/libcef_dll" "$OUT/libcef_dll"
[ -f "$SRC/CMakeLists.txt" ] && ln -sfn "$SRC/CMakeLists.txt" "$OUT/CMakeLists.txt"

cat > "$OUT/archive.json" <<JSON
{
  "type": "standard",
  "name": "cef_binary_${VERSION}_linux64.tar.bz2",
  "sha1": ""
}
JSON

echo "Prepared CEF $VERSION for Rust build in $OUT"
