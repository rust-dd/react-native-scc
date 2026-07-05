#!/bin/bash
set -euo pipefail

# Build kv-ffi for iOS targets and create an xcframework.
#
# Requirements:
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$SCRIPT_DIR/.."
RUST_DIR="$ROOT_DIR/crates/kv-ffi"
TARGET_DIR="$ROOT_DIR/target"
OUT_DIR="$ROOT_DIR/ios/Libs"
HEADER="$ROOT_DIR/cpp/scc_kv_ffi.h"

if command -v cbindgen &>/dev/null; then
  echo "==> Generating C header..."
  cd "$RUST_DIR"
  cbindgen --config cbindgen.toml --crate kv-ffi --output "$HEADER" .
fi

TARGETS_DEVICE="aarch64-apple-ios"
TARGETS_SIM="aarch64-apple-ios-sim x86_64-apple-ios"

echo "==> Building kv-ffi for iOS..."

for target in $TARGETS_DEVICE $TARGETS_SIM; do
  rustup target add "$target" 2>/dev/null || true
done

for target in $TARGETS_DEVICE $TARGETS_SIM; do
  echo "  -> Building $target..."
  cargo build --manifest-path "$RUST_DIR/Cargo.toml" --release --target "$target"
done

mkdir -p "$OUT_DIR"

cp "$TARGET_DIR/$TARGETS_DEVICE/release/libscc_kv_ffi.a" "$OUT_DIR/libscc_kv_ffi-ios.a"

echo "  -> Creating simulator fat library..."
lipo -create \
  "$TARGET_DIR/aarch64-apple-ios-sim/release/libscc_kv_ffi.a" \
  "$TARGET_DIR/x86_64-apple-ios/release/libscc_kv_ffi.a" \
  -output "$OUT_DIR/libscc_kv_ffi-ios-sim.a"

echo "  -> Stripping debug symbols..."
strip -S "$OUT_DIR/libscc_kv_ffi-ios.a"
strip -S "$OUT_DIR/libscc_kv_ffi-ios-sim.a"

echo "  -> Creating xcframework..."
rm -rf "$OUT_DIR/scc_kv_ffi.xcframework"
TMPDIR=$(mktemp -d)
cp "$OUT_DIR/libscc_kv_ffi-ios.a" "$TMPDIR/libscc_kv_ffi.a"
mkdir -p "$TMPDIR/sim"
cp "$OUT_DIR/libscc_kv_ffi-ios-sim.a" "$TMPDIR/sim/libscc_kv_ffi.a"
xcodebuild -create-xcframework \
  -library "$TMPDIR/libscc_kv_ffi.a" \
  -library "$TMPDIR/sim/libscc_kv_ffi.a" \
  -output "$OUT_DIR/scc_kv_ffi.xcframework"
rm -rf "$TMPDIR"

echo "==> iOS build complete!"
echo "    $OUT_DIR/scc_kv_ffi.xcframework"
