#!/bin/sh
# Rebuilds assets/agents/ from pinned sources: the agent logos shown on the
# right of a banner. NOTICE.md records where each one came from and whose
# mark it is; keep the two in step.
#
# Files are copied byte for byte, never resized or recoloured. A logo whose
# colours only read on one background gets a second file in dark/, which
# the plugin uses when macOS is in dark mode.
#
# xAI's zip sits behind a Cloudflare check that curl can't pass, so it has
# to be downloaded in a browser and passed in with --xai-zip.

set -eu

LOBE_VERSION=1.97.1
LOBE_URL="https://registry.npmjs.org/@lobehub/icons-static-png/-/icons-static-png-${LOBE_VERSION}.tgz"
LOBE_SHA256=2e7ad23a41ea3b56104310962fe19b2a2c3bc80766e5ec891c80bcc5b4abb37e

KILO_COMMIT=7d977bce994af36f0edf752cb53e3aefc7aeb214
KILO_PATH=packages/kilo-vscode/assets/icons/kilo-light.png
KILO_URL="https://raw.githubusercontent.com/Kilo-Org/kilocode/${KILO_COMMIT}/${KILO_PATH}"
KILO_SHA256=fa0f39f2409d31fd5e5b132fc9f533d57c879f6e22bdd0f21e250cd987b2ffeb

XAI_URL="https://data.x.ai/logos/SpaceXAI_Grok_Assets.zip"
XAI_SHA256=db9129acd4efc4c2202d25afe31b70281a79f8507f75520ab5e6b3356895a7e9

# label, then the LobeHub file for a light background, then the one for a
# dark background if it differs. The label is the agent name Herdr reports,
# which isn't always LobeHub's name: `copilot.png` there is Microsoft's.
LOBE_MAP="
agy        antigravity-color
amp        amp-color
claude     claude-color
cline      cline            cline
codex      codex-color
copilot    githubcopilot    githubcopilot
cursor     cursor           cursor
devin      devin-color
gemini     gemini-color
hermes     hermesagent      hermesagent
kimi       kimi             kimi
kiro       kiro-color
mastracode mastra           mastra
opencode   opencode         opencode
pi         pi               pi
qodercli   qoder-color      qoder-color
qwen       qwen-color
"

usage() {
    cat <<'EOF'
usage: tools/fetch-icons.sh --xai-zip PATH [--lobe-tgz PATH]

  --xai-zip PATH   xAI's logo pack, downloaded in a browser from
                   https://data.x.ai/logos/SpaceXAI_Grok_Assets.zip
  --lobe-tgz PATH  an already-downloaded LobeHub tarball

Every source is pinned by a sha256 at the top of this script.
EOF
}

die() { printf 'fetch-icons.sh: %s\n' "$*" >&2; exit 1; }

xai_zip=
lobe_tgz=
while [ $# -gt 0 ]; do
    case "$1" in
        --xai-zip) [ $# -ge 2 ] || die "--xai-zip needs a path"; xai_zip=$2; shift 2 ;;
        --lobe-tgz) [ $# -ge 2 ] || die "--lobe-tgz needs a path"; lobe_tgz=$2; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; die "unknown argument: $1" ;;
    esac
done
[ -n "$xai_zip" ] || { usage >&2; die "--xai-zip is required"; }
[ -f "$xai_zip" ] || die "$xai_zip not found"

root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/assets/agents"

work=$(mktemp -d "${TMPDIR:-/tmp}/herdr-nudge-icons.XXXXXX")
trap 'rm -rf "$work"' EXIT

check_hash() {
    have=$(shasum -a 256 "$1" | awk '{ print $1 }')
    [ "$2" = "$have" ] || die "$3: expected sha256 $2, got $have"
}

if [ -n "$lobe_tgz" ]; then
    cp "$lobe_tgz" "$work/lobe.tgz"
else
    curl -fLSs -o "$work/lobe.tgz" "$LOBE_URL" || die "could not download $LOBE_URL"
fi
check_hash "$work/lobe.tgz" "$LOBE_SHA256" "LobeHub $LOBE_VERSION"
tar -xzf "$work/lobe.tgz" -C "$work"

curl -fLSs -o "$work/kilo.png" "$KILO_URL" || die "could not download $KILO_URL"
check_hash "$work/kilo.png" "$KILO_SHA256" "Kilo $KILO_PATH"

check_hash "$xai_zip" "$XAI_SHA256" "xAI logo pack"
unzip -q "$xai_zip" -d "$work/xai"
xai="$work/xai/SpaceXAI_Grok_Assets"

# Built beside the sources and moved into place only once complete, so a
# copy that fails leaves the old icons as they were.
new="$work/agents"
mkdir -p "$new/dark"

echo "$LOBE_MAP" | while read -r label light dark; do
    [ -n "$label" ] || continue
    cp "$work/package/light/$light.png" "$new/$label.png"
    if [ -n "$dark" ]; then
        cp "$work/package/dark/$dark.png" "$new/dark/$label.png"
    fi
done

# An opaque tile, so it reads on either background.
cp "$work/kilo.png" "$new/kilo.png"
# xAI names these by the logo's colour: Dark is the black one, for a light
# background.
cp "$xai/Grok_Logomark_Dark.png" "$new/grok.png"
cp "$xai/Grok_Logomark_Light.png" "$new/dark/grok.png"

rm -rf "$out"
mkdir -p "$(dirname "$out")"
mv "$new" "$out"

# Every file's row in NOTICE.md names its path and its sha256, so a changed
# file means the NOTICE needs a new row.
stale=0
for path in "$out"/*.png "$out"/dark/*.png; do
    rel=${path#"$root/"}
    hash=$(shasum -a 256 "$path" | awk '{ print $1 }')
    if ! grep -F "\`$rel\`" "$root/NOTICE.md" | grep -qF "\`$hash\`"; then
        printf 'NOTICE.md has no row for %s with sha256 %s\n' "$rel" "$hash" >&2
        stale=1
    fi
done
[ "$stale" -eq 0 ] || die "update NOTICE.md for the files above"
echo "assets/agents rebuilt; NOTICE.md matches every file"
