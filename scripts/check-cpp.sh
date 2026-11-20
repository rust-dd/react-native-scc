#!/bin/bash
set -euo pipefail

# Compile-checks cpp/ against the Nitro and JSI headers in node_modules, laid out the way CocoaPods exposes them.
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

mkdir -p "$WORK_DIR/NitroModules"
find "$ROOT_DIR/node_modules/react-native-nitro-modules/cpp" -name '*.hpp' -exec ln -sf {} "$WORK_DIR/NitroModules/" \;
printf '#include "HybridSccKv.hpp"\n#include "HybridSccKvInstance.hpp"\n' > "$WORK_DIR/check.cpp"

"${CXX:-clang++}" -std=c++20 -fsyntax-only \
  -I"$WORK_DIR" \
  -I"$ROOT_DIR/cpp" \
  -I"$ROOT_DIR/nitrogen/generated/shared/c++" \
  -I"$ROOT_DIR/node_modules/react-native/ReactCommon/jsi" \
  "$WORK_DIR/check.cpp"
echo "cpp/ compiles"
