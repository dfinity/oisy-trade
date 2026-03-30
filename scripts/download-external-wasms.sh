#!/usr/bin/env bash
set -euo pipefail

WASMS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../wasms" && pwd)"

# --- Ledger WASM ---
LEDGER_RELEASE="ledger-suite-icrc-2026-03-09"
LEDGER_FILE="ic-icrc1-ledger.wasm.gz"
LEDGER_SHA256="354dd6ecfdc72b5409805b31dea22c9db11df6e14095a5a68924eb63535e6d8a"
LEDGER_URL="https://github.com/dfinity/ic/releases/download/${LEDGER_RELEASE}/${LEDGER_FILE}"

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        echo "ERROR: No SHA-256 tool found (need sha256sum or shasum)" >&2
        exit 1
    fi
}

download_if_needed() {
    local url="$1"
    local dest="$2"
    local expected_sha256="$3"

    if [ -f "$dest" ]; then
        actual_sha256=$(sha256 "$dest")
        if [ "$actual_sha256" = "$expected_sha256" ]; then
            echo "Already downloaded: $(basename "$dest")"
            return 0
        fi
        echo "Hash mismatch for $(basename "$dest"), re-downloading..."
    fi

    echo "Downloading $(basename "$dest")..."
    curl -fsSL -o "$dest" "$url"

    actual_sha256=$(sha256 "$dest")
    if [ "$actual_sha256" != "$expected_sha256" ]; then
        echo "ERROR: SHA-256 verification failed for $(basename "$dest")"
        echo "  Expected: $expected_sha256"
        echo "  Actual:   $actual_sha256"
        rm -f "$dest"
        exit 1
    fi
    echo "Downloaded and verified: $(basename "$dest")"
}

mkdir -p "$WASMS_DIR"
download_if_needed "$LEDGER_URL" "${WASMS_DIR}/${LEDGER_FILE}" "$LEDGER_SHA256"
