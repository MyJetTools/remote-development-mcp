#!/usr/bin/env bash
set -euo pipefail

# Builds the browser console and drops it into ./wwwroot, which is where the
# server's static-files middleware reads from.
#
# Run it from anywhere — every path below is resolved from this script's own
# location, not from the current directory.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UI_DIR="${SCRIPT_DIR}/remote-development-mcp-ui"
WWWROOT="${SCRIPT_DIR}/wwwroot"
DX_OUT="${UI_DIR}/target/dx/remote-development-mcp-ui/release/web/public"

if ! command -v dx >/dev/null 2>&1; then
    echo "ERROR: 'dx' is not installed — the wasm console can not be built without it."
    echo "       cargo install dioxus-cli"
    exit 1
fi

cd "${UI_DIR}"

# dx names its assets by content hash and only ever adds to this folder, so
# without wiping it first the copy below carries every bundle ever built into
# wwwroot — index.html points at one of them and the rest are dead megabytes.
# Wiping wwwroot alone does not help: the staleness is on this side of the copy.
echo ">> cleaning ${UI_DIR}/target/dx"
rm -rf "${UI_DIR}/target/dx"

echo ">> dx build --release --web"
dx build --release --web

if [ ! -d "${DX_OUT}" ]; then
    echo "ERROR: build output not found at ${DX_OUT}"
    exit 1
fi

# Wiped rather than overwritten: a stale hashed asset left behind from an
# earlier build is still reachable, and index.html no longer points at it.
echo ">> cleaning ${WWWROOT}"
rm -rf "${WWWROOT}"
mkdir -p "${WWWROOT}"

echo ">> copying into ${WWWROOT}"
cp -R "${DX_OUT}/." "${WWWROOT}/"

echo ">> done — restart the server, then open the address it prints."
