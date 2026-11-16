#!/usr/bin/env bash
# Monolith tool installer for Linux and Git Bash on Windows.
# Usage: tools/install.sh [--add-to-path | --uninstall | --help]
#   no flag        build the tools (cargo build --release), copy them to ~/.monolith/bin, build the
#                  Athena web app into ~/.monolith/share/athena-web (for `athena serve`), build the
#                  Titan kernel into ~/.monolith/share/titan (for `titan boot`; a failure only warns),
#                  write ~/.monolith/VERSION and MANIFEST, print the PATH line. PATH is not changed.
#   --add-to-path  also append ONE guarded line to ~/.profile (skipped if already there).
#   --uninstall    remove ~/.monolith/bin, share/athena-web, share/titan, VERSION, MANIFEST and the ~/.profile line.
# Re-running upgrades in place. A tool that cannot be replaced (in use) is reported at the end.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MONO="$HOME/.monolith"
BIN_DIR="$MONO/bin"
PROFILE="$HOME/.profile"
MARK="# monolith-path"

# package:bin pairs (the allowlist). Not installed, on purpose:
#   deploy: argo-registry, aurum-node, iridium-server, monolith-web (servers needing setup), healthcheck (container probe)
#   zeus `zeus` and titan `kernel`/`userland`: no_std images, empty stubs on a host
#   silicon crate `silicon`: same bin name as chisel's `silicon`, which is the one installed
TOOLS="sphinx:sphinx midas:midas kairos:kairos hades:hades chisel:silicon elysium:elysium
cobalt:cobalt cobalt:logos xenon:xenon mercury:mercury iridium:iridium iridium:iridium-cli
chronos:chronos argo:argo aurum:aurum krypton:krypton calliope:calliope indium:indium
iris:iris odyssey:odyssey helios:helios pandora:pandora lithium:lithium zeus:zeusctl
bench:bench athena:athena neon:titan aci:aci"

MODE=install
ADD=0
for a in "$@"; do
  case "$a" in
    --add-to-path) ADD=1 ;;
    --uninstall) MODE=uninstall ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "install.sh: unknown option '$a' (try --help)" >&2; exit 2 ;;
  esac
done

# One-line summary of a tool: the first "name - text" line of its --help, else its first line.
summary() {
  local out line
  out="$("$1" --help </dev/null 2>/dev/null || true)"
  line="$(printf '%s\n' "$out" | awk 'NF' | head -n 6 | grep -m1 '^[^[:space:]].* - ' || true)"
  [ -n "$line" ] || line="$(printf '%s\n' "$out" | awk 'NF' | head -n 1 || true)"
  printf '%s' "${line:-no --help output}"
}

uninstall() {
  if [ -f "$PROFILE" ] && grep -qF "$MARK" "$PROFILE"; then
    grep -vF "$MARK" "$PROFILE" > "$PROFILE.monolith-tmp" || true
    mv -f "$PROFILE.monolith-tmp" "$PROFILE"
    echo "removed the PATH line from $PROFILE"
  fi
  if [ -d "$BIN_DIR" ]; then
    rm -rf -- "$BIN_DIR" || { echo "could not remove $BIN_DIR (a tool is still running?)" >&2; exit 1; }
    echo "removed $BIN_DIR"
  fi
  rm -rf -- "$MONO/share/athena-web"
  rm -rf -- "$MONO/share/titan"
  rmdir "$MONO/share" 2>/dev/null || true
  rm -f -- "$MONO/VERSION" "$MONO/MANIFEST"
  rmdir "$MONO" 2>/dev/null || true
  echo "Monolith tools uninstalled."
}

install() {
  command -v cargo >/dev/null 2>&1 || { echo "install.sh: cargo not found; install Rust from https://rustup.rs first" >&2; exit 1; }
  EXE=""
  case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) EXE=.exe ;; esac
  TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"

  bins=()
  for t in $TOOLS; do
    bin="${t##*:}"
    [ "$bin" = silicon ] || bins+=(--bin "$bin")   # silicon is built on its own below
  done
  echo "building ${#bins[@]} tools in release mode (first run is slow)..."
  cargo build --release --manifest-path "$ROOT/Cargo.toml" "${bins[@]}"
  cargo build --release --manifest-path "$ROOT/Cargo.toml" -p chisel --bin silicon

  # the web app that `athena serve` finds at ../share/athena-web; the pack writes ROOT/target/athena-web
  echo "building the Athena web app (wasm and riscv targets needed)..."
  if ! cargo run -q --release --manifest-path "$ROOT/Cargo.toml" -p athena --example pack >/dev/null; then
    echo "install.sh: web pack failed; nothing was copied (check: rustup target add wasm32-unknown-unknown riscv64gc-unknown-none-elf)" >&2
    exit 1
  fi

  mkdir -p "$BIN_DIR"
  busy=()
  for t in $TOOLS; do
    bin="${t##*:}"
    src="$TARGET/release/$bin$EXE"
    dst="$BIN_DIR/$bin$EXE"
    # copy to a sibling, then rename over the old file: a tool that is running stays intact and is reported
    if err="$( { cp -f -- "$src" "$dst.new" && mv -f -- "$dst.new" "$dst"; } 2>&1 )"; then
      :
    else
      rm -f -- "$dst.new"
      busy+=("$bin ($err)")
    fi
  done

  # replace the web app as a whole, so a file removed from the pack does not linger
  mkdir -p "$MONO/share"
  rm -rf -- "$MONO/share/athena-web"
  cp -R -- "$ROOT/target/athena-web" "$MONO/share/athena-web"

  # the kernel that `titan boot` finds at ../share/titan/kernel; if it cannot be built, only boot is affected
  echo "building the Titan kernel (riscv64 target needed)..."
  if ( cd "$ROOT" && cargo build --target-dir target/titan-kernel -p titan --bin kernel --release --target riscv64gc-unknown-none-elf --manifest-path "$ROOT/Cargo.toml" ) \
    && rm -rf -- "$MONO/share/titan" && mkdir -p "$MONO/share/titan" \
    && cp -f -- "$ROOT/target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel" "$MONO/share/titan/kernel"; then
    :
  else
    echo "install.sh: warning: Titan kernel not installed; titan boot needs --kernel (check: rustup target add riscv64gc-unknown-none-elf)" >&2
  fi

  SHA="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
  printf '%s\n' "$SHA" > "$MONO/VERSION"
  {
    printf '# Monolith %s, installed %s: tool<TAB>one-line --help summary\n' "$SHA" "$(date -u +%Y-%m-%d)"
    for t in $TOOLS; do
      bin="${t##*:}"
      [ -f "$BIN_DIR/$bin$EXE" ] && printf '%s\t%s\n' "$bin" "$(summary "$BIN_DIR/$bin$EXE")"
    done
  } > "$MONO/MANIFEST.new"
  mv -f "$MONO/MANIFEST.new" "$MONO/MANIFEST"

  count=$(ls "$BIN_DIR" | grep -c . || true)
  echo "installed $count tools into $BIN_DIR (version $SHA)"

  line='case ":$PATH:" in *":$HOME/.monolith/bin:"*) ;; *) export PATH="$HOME/.monolith/bin:$PATH" ;; esac '"$MARK"
  if [ "$ADD" = 1 ]; then
    if [ -f "$PROFILE" ] && grep -qF "$MARK" "$PROFILE"; then
      echo "PATH: already set in $PROFILE (nothing changed)"
    else
      printf '%s\n' "$line" >> "$PROFILE"
      echo "PATH: appended one guarded line to $PROFILE; open a new shell to use the tools"
    fi
  else
    echo 'PATH is unchanged. To use the tools everywhere, add this line to ~/.profile:'
    echo '  export PATH="$HOME/.monolith/bin:$PATH"'
    echo '(or re-run with --add-to-path to have it appended for you)'
  fi

  if [ "${#busy[@]}" -gt 0 ]; then
    echo "NOT UPDATED (in use, close them and re-run): ${busy[*]}" >&2
    exit 1
  fi
}

if [ "$MODE" = uninstall ]; then uninstall; else install; fi
