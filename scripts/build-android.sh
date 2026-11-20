#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RUST_DIR="$ROOT_DIR/crates/kv-ffi"
OUT_DIR="$ROOT_DIR/android/src/main/jniLibs"
HEADER="$ROOT_DIR/cpp/scc_kv_ffi.h"
STAMP="$ROOT_DIR/android/rust-sources.sha256"
ABIS="arm64-v8a armeabi-v7a x86 x86_64"

source "$SCRIPT_DIR/rust-stamp.sh"
OUTPUTS=()
for abi in $ABIS; do OUTPUTS+=("$OUT_DIR/$abi/libscc_kv_ffi.a"); done
handle_mode "${1:-}" "$STAMP" "${OUTPUTS[@]}"

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$PWD/$TARGET_DIR" ;; esac
export CARGO_TARGET_DIR="$TARGET_DIR"

if command -v cbindgen &>/dev/null; then
  echo "==> Generating C header..."
  cd "$RUST_DIR"
  cbindgen --config cbindgen.toml --crate kv-ffi --output "$HEADER" .
fi

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  for sdk in "${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}" "$HOME/Library/Android/sdk" "$HOME/Android/Sdk"; do
    [ -n "$sdk" ] && [ -d "$sdk/ndk" ] || continue
    newest="$(find "$sdk/ndk" -mindepth 1 -maxdepth 1 -type d ! -name '.*' | sort -V | tail -1)"
    if [ -n "$newest" ]; then
      ANDROID_NDK_HOME="$newest"
      break
    fi
  done
fi

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  echo "ERROR: Cannot find Android NDK. Set ANDROID_NDK_HOME or install NDK via Android Studio."
  exit 1
fi

export ANDROID_NDK_HOME
echo "==> Using NDK: $ANDROID_NDK_HOME"
LLVM_STRIP="$(find "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt" -path '*/bin/llvm-strip' 2>/dev/null | head -1)"
echo "==> Building kv-ffi for Android..."

if ! command -v cargo-ndk &>/dev/null; then
  echo "  -> Installing cargo-ndk..."
  cargo install cargo-ndk
fi

TARGETS="aarch64-linux-android x86_64-linux-android armv7-linux-androideabi i686-linux-android"

for target in $TARGETS; do
  rustup target add "$target" 2>/dev/null || true
done

build_abi() {
  local abi=$1
  local target=$2
  echo "  -> Building $abi ($target)..."
  cd "$RUST_DIR"
  cargo ndk \
    --target "$target" \
    --platform 24 \
    -- build --release
  mkdir -p "$OUT_DIR/$abi"
  cp "$TARGET_DIR/$target/release/libscc_kv_ffi.a" "$OUT_DIR/$abi/"
  if [ -n "$LLVM_STRIP" ]; then
    "$LLVM_STRIP" --strip-debug "$OUT_DIR/$abi/libscc_kv_ffi.a"
  else
    echo "  !! llvm-strip not found in the NDK; $abi keeps its debug info"
  fi
}

build_abi "arm64-v8a" "aarch64-linux-android"
build_abi "x86_64" "x86_64-linux-android"
build_abi "armeabi-v7a" "armv7-linux-androideabi"
build_abi "x86" "i686-linux-android"

# `strip = "symbols"` does not apply to staticlibs; the NDK's llvm-strip above does, while macOS `strip` would corrupt the ELF archives.
rust_sources_hash > "$STAMP"

echo "==> Android build complete!"
