# Sourced by the build scripts: prebuilt libraries are reused only when they were built from the current Rust sources.

rust_sources_hash() {
  local hasher="shasum -a 256"
  command -v shasum >/dev/null 2>&1 || hasher="sha256sum"
  (
    cd "$ROOT_DIR"
    {
      find crates/kv-core/src crates/kv-ffi/src -type f -name '*.rs'
      printf '%s\n' crates/kv-core/Cargo.toml crates/kv-ffi/Cargo.toml Cargo.toml Cargo.lock
    } | LC_ALL=C sort | while IFS= read -r file; do
      printf '%s\n' "$file"
      cat "$file"
    done | $hasher | cut -d' ' -f1
  )
}

outputs_fresh() {
  local stamp=$1
  shift
  [ -f "$stamp" ] || return 1
  for output in "$@"; do
    [ -e "$output" ] || return 1
  done
  [ "$(cat "$stamp")" = "$(rust_sources_hash)" ]
}

# --check exits 0 when fresh and 1 when stale, without needing cargo; --if-needed builds only when stale.
handle_mode() {
  local mode=$1 stamp=$2
  shift 2
  case "$mode" in
    --check)
      outputs_fresh "$stamp" "$@" && exit 0
      exit 1
      ;;
    --if-needed)
      if outputs_fresh "$stamp" "$@"; then exit 0; fi
      ;;
    "") ;;
    *)
      echo "usage: $0 [--check|--if-needed]" >&2
      exit 2
      ;;
  esac
  rm -f "$stamp"
}
