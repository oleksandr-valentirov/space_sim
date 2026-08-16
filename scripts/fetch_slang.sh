#!/bin/sh
# Download the Slang compiler (ROADMAP P1).
#
# Unlike the JPL data this is NOT committed: 24 MB of binaries updated monthly.
# Hence a script rather than a file in the repository, and hence tools/slang/
# in .gitignore.
#
# Why Slang at all -- PROJECT.md section 7: modules, generics and one source
# text for several targets instead of three copies of WGSL. Question P1 is
# whether its output reaches wgpu, and by which route:
#
#   via WGSL     slangc -target wgsl    -> naga parses it as a normal shader
#   via SPIR-V   slangc -target spirv   -> passthrough, only where the backend
#                                          supports it
#
# tools/slang-probe checks both.
#
#   sh scripts/fetch_slang.sh            latest release
#   sh scripts/fetch_slang.sh v2026.14.1 a specific one
#
# Run from the repository root.

set -eu

OUT_DIR="tools/slang"
TAG="${1:-}"

# The tag is pinned in the output, not just in the argument: otherwise "works
# on mine" and "fails in CI" cannot be reconciled, because the versions differ
# and are recorded nowhere.
#
# The tag comes from the releases/latest redirect, NOT from api.github.com.
# The API without a token allows 60 requests per hour per IP, and macOS runners
# share a few NAT addresses across all jobs, so the quota there is regularly
# spent by other people's builds. It looked like this exact step failing in
# half a second while linux in the same run passed, as had macOS twenty minutes
# earlier. The redirect lives on github.com and has no such quota.
if [ -z "$TAG" ]; then
    # The status stays in the error output deliberately. The previous version
    # caught only "tag is empty" and said nothing about why: `curl -sS` without
    # --fail reports nothing about a 403, the response body simply lacks the
    # field.
    RESOLVED=$(curl -sSL -o /dev/null \
               -w '%{http_code} %{url_effective}' \
               "https://github.com/shader-slang/slang/releases/latest" || true)

    case "$RESOLVED" in
        "200 "*"/releases/tag/"*) TAG=${RESOLVED##*/} ;;
        *)
            echo "could not resolve the latest tag: ${RESOLVED:-curl did not answer}" >&2
            echo "pass one explicitly, e.g.: sh scripts/fetch_slang.sh v2026.14.1" >&2
            exit 1 ;;
    esac
fi

VERSION=${TAG#v}

case "$(uname -s)" in
    Linux)  OS=linux ;;
    Darwin) OS=macos ;;
    *)      echo "unsupported OS: $(uname -s). On Windows take the zip by hand" >&2
            exit 1 ;;
esac

case "$(uname -m)" in
    x86_64|amd64)  ARCH=x86_64 ;;
    arm64|aarch64) ARCH=aarch64 ;;
    *)             echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

ASSET="slang-${VERSION}-${OS}-${ARCH}.tar.gz"
URL="https://github.com/shader-slang/slang/releases/download/${TAG}/${ASSET}"

echo "Slang ${TAG}, ${OS}-${ARCH}"
echo "  $URL"

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

curl -sSL --fail "$URL" | tar -xz -C "$OUT_DIR"

if [ ! -x "$OUT_DIR/bin/slangc" ]; then
    echo "no bin/slangc in the archive -- did the release layout change?" >&2
    exit 1
fi

# Version to disk: the probe reads it and prints it in the table, so the P1
# result is tied to a specific compiler rather than "whatever was installed".
echo "$TAG" > "$OUT_DIR/VERSION"

echo ""
"$OUT_DIR/bin/slangc" -v 2>&1 | head -2 || true
echo ""
echo "Done: $OUT_DIR/bin/slangc"
