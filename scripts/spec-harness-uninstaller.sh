#!/bin/sh
# Uninstaller for spec-harness.
#
# Reads the install receipt written by spec-harness-installer.sh, removes the
# installed binaries and the receipt, and reports anything it leaves behind.
#
# Usage:
#   ./spec-harness-uninstaller.sh        # asks for confirmation
#   ./spec-harness-uninstaller.sh -y     # no confirmation prompt
set -u

APP_NAME="spec-harness"

say() { echo "$1"; }
err() {
    echo "ERROR: $1" >&2
    exit 1
}

ASSUME_YES=0
if [ "${1:-}" = "-y" ] || [ "${1:-}" = "--yes" ]; then
    ASSUME_YES=1
fi

# Locate the receipt the same way the installer stored it: LOCALAPPDATA on
# Windows POSIX shells, XDG config dir everywhere else.
case "$(uname)" in
    CYGWIN*|MSYS*|MINGW*)
        RECEIPT_HOME="${LOCALAPPDATA:-${XDG_CONFIG_HOME:-$HOME/.config}}/$APP_NAME"
        ;;
    *)
        RECEIPT_HOME="${XDG_CONFIG_HOME:-$HOME/.config}/$APP_NAME"
        ;;
esac
RECEIPT="$RECEIPT_HOME/$APP_NAME-receipt.json"

if [ ! -f "$RECEIPT" ]; then
    err "no install receipt found at $RECEIPT — was $APP_NAME installed with the installer script?"
fi

# Pull the binaries array and install prefix out of the single-line receipt
# JSON without requiring jq.
BINS=$(sed 's/.*"binaries":\[\([^]]*\)\].*/\1/' "$RECEIPT" | tr -d '"' | tr ',' ' ')
PREFIX=$(sed 's/.*"install_prefix":"\([^"]*\)".*/\1/' "$RECEIPT")

if [ -z "$BINS" ] || [ -z "$PREFIX" ]; then
    err "could not parse binaries and install_prefix from $RECEIPT"
fi

# Binaries live in $PREFIX/bin for the CARGO_HOME layout the installer uses,
# but fall back to $PREFIX itself for flat layouts.
TARGETS=""
for BIN in $BINS; do
    if [ -f "$PREFIX/bin/$BIN" ]; then
        TARGETS="$TARGETS $PREFIX/bin/$BIN"
    elif [ -f "$PREFIX/$BIN" ]; then
        TARGETS="$TARGETS $PREFIX/$BIN"
    else
        say "warning: $BIN not found under $PREFIX (already removed?)"
    fi
done

say "This will remove:"
for TARGET in $TARGETS; do
    say "  $TARGET"
done
say "  $RECEIPT"

if [ "$ASSUME_YES" -ne 1 ]; then
    printf "Proceed? [y/N] "
    ANSWER=""
    # Read from the terminal even when the script itself is piped into sh
    # (curl ... | sh), where stdin is the script stream. With no terminal
    # available at all, ANSWER stays empty and we abort below.
    if [ -t 0 ]; then
        read -r ANSWER
    else
        ANSWER=$( (read -r LINE < /dev/tty && printf '%s' "$LINE") 2>/dev/null ) || ANSWER=""
    fi
    case "$ANSWER" in
        y|Y|yes|YES) ;;
        *) say "aborted (run with -y to skip the prompt)"; exit 0 ;;
    esac
fi

for TARGET in $TARGETS; do
    rm -f "$TARGET" || err "failed to remove $TARGET"
    say "removed $TARGET"
done

rm -f "$RECEIPT"
rmdir "$RECEIPT_HOME" 2>/dev/null || true
say "removed install receipt"

# The installer may have added `. "$HOME/.cargo/env"` to a shell profile to
# put the install dir on PATH. That file is shared with rustup, so removing
# it automatically could break a working Rust toolchain — just mention it.
case "$PREFIX" in
    *".cargo")
        say ""
        say "note: if you do not use Rust/rustup and your shell profile sources"
        say "\"\$HOME/.cargo/env\", you can remove that line manually."
        ;;
esac

say "$APP_NAME uninstalled"
