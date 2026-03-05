#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/package.sh [--target <triple>] [--out-dir <dir>] [--no-strip]

Build and package mouse-sounds into a .tar.gz bundle.

Options:
  --target <triple>   Rust target triple, e.g. x86_64-unknown-linux-gnu
  --out-dir <dir>     Output directory for archives (default: ./dist)
  --no-strip          Do not strip the packaged binary
  -h, --help          Show this help message
USAGE
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="$PROJECT_ROOT/dist"
TARGET=""
STRIP_BINARY=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      [[ $# -ge 2 ]] || { echo "error: --target requires a value" >&2; exit 1; }
      TARGET="$2"
      shift 2
      ;;
    --out-dir)
      [[ $# -ge 2 ]] || { echo "error: --out-dir requires a value" >&2; exit 1; }
      OUT_DIR="$2"
      shift 2
      ;;
    --no-strip)
      STRIP_BINARY=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown option '$1'" >&2
      usage
      exit 1
      ;;
  esac
done

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "error: required command not found: $1" >&2
    exit 1
  }
}

require_cmd cargo
require_cmd tar
require_cmd sha256sum

cd "$PROJECT_ROOT"

PACKAGE_NAME="$(awk -F ' = ' '/^name = / { gsub(/"/, "", $2); print $2; exit }' Cargo.toml)"
PACKAGE_VERSION="$(awk -F ' = ' '/^version = / { gsub(/"/, "", $2); print $2; exit }' Cargo.toml)"

if [[ -z "$PACKAGE_NAME" || -z "$PACKAGE_VERSION" ]]; then
  echo "error: failed to read package name/version from Cargo.toml" >&2
  exit 1
fi

if [[ -n "$TARGET" ]]; then
  cargo build --release --locked --target "$TARGET"
  TARGET_LABEL="$TARGET"
  BUILD_BIN="target/$TARGET/release/$PACKAGE_NAME"
else
  cargo build --release --locked
  TARGET_LABEL="$(rustc -vV | awk -F ': ' '/^host:/ {print $2}')"
  BUILD_BIN="target/release/$PACKAGE_NAME"
fi

if [[ "$TARGET_LABEL" == *windows* ]]; then
  BUILD_BIN="${BUILD_BIN}.exe"
  BIN_NAME="${PACKAGE_NAME}.exe"
else
  BIN_NAME="$PACKAGE_NAME"
fi

if [[ ! -f "$BUILD_BIN" ]]; then
  echo "error: build artifact not found: $BUILD_BIN" >&2
  exit 1
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

BUNDLE_DIR_NAME="${PACKAGE_NAME}-${PACKAGE_VERSION}-${TARGET_LABEL}"
BUNDLE_ROOT="$WORK_DIR/$BUNDLE_DIR_NAME"

mkdir -p "$BUNDLE_ROOT/bin"
mkdir -p "$BUNDLE_ROOT/share/$PACKAGE_NAME"
mkdir -p "$BUNDLE_ROOT/config"
mkdir -p "$BUNDLE_ROOT/systemd/user"

cp "$BUILD_BIN" "$BUNDLE_ROOT/bin/$BIN_NAME"

if [[ "$STRIP_BINARY" -eq 1 ]] && command -v strip >/dev/null 2>&1; then
  strip "$BUNDLE_ROOT/bin/$BIN_NAME" || true
fi

cp README.md "$BUNDLE_ROOT/"
[[ -f click_down.wav ]] && cp click_down.wav "$BUNDLE_ROOT/share/$PACKAGE_NAME/"
[[ -f click_up.wav ]] && cp click_up.wav "$BUNDLE_ROOT/share/$PACKAGE_NAME/"

cat > "$BUNDLE_ROOT/config/config.toml" <<CFG
[sounds]
down = "/home/your_user/.local/share/${PACKAGE_NAME}/click_down.wav"
up = "/home/your_user/.local/share/${PACKAGE_NAME}/click_up.wav"

[device]
# Keep empty to auto-detect all readable mouse devices.
# event_path = "/dev/input/event12"

[behavior]
all_buttons = true
CFG

cat > "$BUNDLE_ROOT/systemd/user/${PACKAGE_NAME}.service" <<SERVICE
[Unit]
Description=Mouse click sound daemon
After=graphical-session.target
Wants=graphical-session.target

[Service]
ExecStart=%h/.local/bin/${PACKAGE_NAME} run --config %h/.config/${PACKAGE_NAME}/config.toml
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
SERVICE

cat > "$BUNDLE_ROOT/install.sh" <<'INSTALL'
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$HOME/.local/bin"
SHARE_DIR="$HOME/.local/share/mouse-sounds"
CONF_DIR="$HOME/.config/mouse-sounds"
SYSTEMD_DIR="$HOME/.config/systemd/user"

mkdir -p "$BIN_DIR" "$SHARE_DIR" "$CONF_DIR" "$SYSTEMD_DIR"
install -m 0755 "$ROOT_DIR/bin/mouse-sounds" "$BIN_DIR/mouse-sounds"

if [[ -f "$ROOT_DIR/share/mouse-sounds/click_down.wav" ]]; then
  install -m 0644 "$ROOT_DIR/share/mouse-sounds/click_down.wav" "$SHARE_DIR/click_down.wav"
fi
if [[ -f "$ROOT_DIR/share/mouse-sounds/click_up.wav" ]]; then
  install -m 0644 "$ROOT_DIR/share/mouse-sounds/click_up.wav" "$SHARE_DIR/click_up.wav"
fi

if [[ ! -f "$CONF_DIR/config.toml" ]]; then
  cat > "$CONF_DIR/config.toml" <<CFG
[sounds]
down = "$SHARE_DIR/click_down.wav"
up = "$SHARE_DIR/click_up.wav"

[device]
# Keep empty to auto-detect all readable mouse devices.
# event_path = "/dev/input/event12"

[behavior]
all_buttons = true
CFG
  echo "wrote default config: $CONF_DIR/config.toml"
else
  echo "config exists, skipped: $CONF_DIR/config.toml"
fi

install -m 0644 "$ROOT_DIR/systemd/user/mouse-sounds.service" "$SYSTEMD_DIR/mouse-sounds.service"

echo "installed mouse-sounds to $HOME/.local"
echo "next: systemctl --user daemon-reload"
echo "next: systemctl --user enable --now mouse-sounds.service"
INSTALL

chmod +x "$BUNDLE_ROOT/install.sh"

mkdir -p "$OUT_DIR"
ARCHIVE_PATH="$OUT_DIR/${BUNDLE_DIR_NAME}.tar.gz"

tar -C "$WORK_DIR" -czf "$ARCHIVE_PATH" "$BUNDLE_DIR_NAME"
sha256sum "$ARCHIVE_PATH" > "${ARCHIVE_PATH}.sha256"

echo "package created: $ARCHIVE_PATH"
echo "checksum: ${ARCHIVE_PATH}.sha256"
