#!/bin/sh
# Builds vendor/HerdrNudge.app: terminal-notifier with our own bundle id,
# name and icon, ad-hoc signed.
#
# The upstream app as it ships refuses to notify — "Notifications are not
# allowed for this application", exit 3. macOS grants notification permission
# per bundle id, and upstream's is signed by someone else. Giving it our own
# id and re-signing is what fixes it.
#
# The id below is permanent. The user's notification grant and the icon macOS
# caches are both keyed on it, so changing it later turns off their
# notifications with no error and no prompt.
#
# Run this by hand, not from CI. vendor/HerdrNudge.app is committed, because
# `herdr plugin install` is a shallow git fetch and the tree is what the user
# gets. Re-running re-signs the bundle, which may void every existing user's
# notification grant, so only do it for a new terminal-notifier or a new icon.

set -eu

BUNDLE_ID=io.github.justinchiasson.herdr-nudge
BUNDLE_NAME="Herdr Nudge"
ICON_NAME=HerdrNudge

TN_VERSION=3.1.0
TN_URL="https://github.com/julienXX/terminal-notifier/releases/download/${TN_VERSION}/terminal-notifier-${TN_VERSION}.zip"
TN_ZIP="terminal-notifier-${TN_VERSION}.zip"
TN_SHA256=e969d4ae20287da1ba55495ae31dcedd8e9069deb8ce4eed24f6561a5fc3e4d5

LOGO_URL="https://herdr.dev/assets/logo.png"
LOGO_NAME="herdr-logo.png"
LOGO_SHA256=56fc2db845c16eb521022549890fbe239659957caee1f4fc718a634d7a66cf0a

# Only used when the release zip carries no licence of its own.
LICENSE_URL="https://raw.githubusercontent.com/julienXX/terminal-notifier/master/LICENSE.md"

LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
PLISTBUDDY=/usr/libexec/PlistBuddy

usage() {
    cat <<'EOF'
usage: tools/bundle/build.sh [options]

  --zip PATH     use an already-downloaded terminal-notifier zip
  --logo PATH    use a local PNG instead of downloading the Herdr logo
  --license PATH use a local copy of terminal-notifier's licence
  --keep-work   leave the scratch directory behind
  -h, --help    this

Downloads are pinned by the sha256 constants at the top of this script. A
mismatch stops the build; update the constant only when you meant to change
the version.
EOF
}

say() { printf '\n== %s\n' "$*"; }
die() { printf 'build.sh: %s\n' "$*" >&2; exit 1; }

zip_src=
logo_src=
license_src=
keep_work=0
while [ $# -gt 0 ]; do
    case "$1" in
        --zip) [ $# -ge 2 ] || die "--zip needs a path"; zip_src=$2; shift 2 ;;
        --logo) [ $# -ge 2 ] || die "--logo needs a path"; logo_src=$2; shift 2 ;;
        --license) [ $# -ge 2 ] || die "--license needs a path"; license_src=$2; shift 2 ;;
        --keep-work) keep_work=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; die "unknown argument: $1" ;;
    esac
done

[ "$(uname -s)" = Darwin ] || die "macOS only"

root=$(cd "$(dirname "$0")/../.." && pwd)
ASSETS="$root/assets"
APP="$root/vendor/$ICON_NAME.app"

for tool in curl unzip sips iconutil codesign shasum xattr awk; do
    command -v "$tool" >/dev/null 2>&1 || die "$tool not found"
done
[ -x "$PLISTBUDDY" ] || die "$PLISTBUDDY not found"

work=$(mktemp -d "${TMPDIR:-/tmp}/herdr-nudge-bundle.XXXXXX")
cleanup() { [ "$keep_work" -eq 1 ] || rm -rf "$work"; }
trap cleanup EXIT

check_hash() {
    path=$1 want=$2 name=$3
    have=$(shasum -a 256 "$path" | awk '{ print $1 }')
    [ "$want" = "$have" ] || die "$name: expected $want, got $have"
    printf '   sha256 ok  %s\n' "$name"
}

say "terminal-notifier $TN_VERSION"
if [ -n "$zip_src" ]; then
    [ -f "$zip_src" ] || die "$zip_src not found"
    cp "$zip_src" "$work/$TN_ZIP"
else
    printf '   downloading %s\n' "$TN_URL"
    curl -fLSs -o "$work/$TN_ZIP" "$TN_URL" \
        || die "download failed. Check the release still exists, or pass --zip with a local copy."
fi
check_hash "$work/$TN_ZIP" "$TN_SHA256" "$TN_ZIP"

unzip -q "$work/$TN_ZIP" -d "$work/unpacked"
src_app=$(find "$work/unpacked" -maxdepth 3 -name 'terminal-notifier.app' -print -quit)
[ -n "$src_app" ] || die "no terminal-notifier.app inside $TN_ZIP"

say "Herdr logo"
mkdir -p "$ASSETS"
if [ -n "$logo_src" ]; then
    [ -f "$logo_src" ] || die "$logo_src not found"
    cp "$logo_src" "$ASSETS/$LOGO_NAME"
elif [ ! -f "$ASSETS/$LOGO_NAME" ]; then
    printf '   downloading %s\n' "$LOGO_URL"
    curl -fLSs -o "$ASSETS/$LOGO_NAME" "$LOGO_URL" \
        || die "download failed. Save the logo yourself and pass --logo."
else
    printf '   using assets/%s\n' "$LOGO_NAME"
fi
check_hash "$ASSETS/$LOGO_NAME" "$LOGO_SHA256" "$LOGO_NAME"
printf '   %s\n' "$(sips -g pixelWidth -g pixelHeight "$ASSETS/$LOGO_NAME" | tr '\n' ' ')"

say "building $ICON_NAME.icns"
# sips -z stretches to the size it is given, so a source that isn't square
# comes out distorted. The Herdr logo is 512x512.
iconset="$work/$ICON_NAME.iconset"
mkdir -p "$iconset"
for s in 16 32 128 256 512; do
    sips -z "$s" "$s" "$ASSETS/$LOGO_NAME" --out "$iconset/icon_${s}x${s}.png" >/dev/null
    if [ $((s * 2)) -le 512 ]; then
        sips -z $((s * 2)) $((s * 2)) "$ASSETS/$LOGO_NAME" \
            --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
    fi
done
iconutil -c icns "$iconset" -o "$work/$ICON_NAME.icns"

say "assembling $APP"
mkdir -p "$root/vendor"
rm -rf "$APP"
cp -R "$src_app" "$APP"
# A downloaded app carries a quarantine flag, which makes Gatekeeper prompt
# the first time anything launches it — including macOS relaunching it for a
# notification click.
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true
cp "$work/$ICON_NAME.icns" "$APP/Contents/Resources/$ICON_NAME.icns"
# Upstream's icon is unused once CFBundleIconFile names ours, and it would sit
# in the repo forever. Removed before signing, since the signature seals the
# file list.
find "$APP/Contents/Resources" -name 'Terminal.icns' -delete

# terminal-notifier is MIT, so its copyright notice has to ship with the
# binary we redistribute. Stop rather than quietly skip it: shipping without
# it is a licence breach, and an empty `|| true` here already hid the problem
# once.
license_out="$root/vendor/terminal-notifier-LICENSE.md"
if [ -n "$license_src" ]; then
    [ -f "$license_src" ] || die "$license_src not found"
    cp "$license_src" "$license_out"
else
    found=$(find "$work/unpacked" \( -iname 'LICENSE*' -o -iname 'COPYING*' \) -print -quit)
    if [ -n "$found" ]; then
        cp "$found" "$license_out"
    else
        printf '   not in the zip, downloading %s\n' "$LICENSE_URL"
        curl -fLSs -o "$license_out" "$LICENSE_URL" || die "could not fetch terminal-notifier's
licence. It is MIT and must ship with the binary. Save it from
  https://github.com/julienXX/terminal-notifier
and re-run with --license <path>."
    fi
fi
[ -s "$license_out" ] || die "$license_out is empty"
printf '   licence  %s\n' "$(basename "$license_out")"

plist="$APP/Contents/Info.plist"
set_string() {
    "$PLISTBUDDY" -c "Set :$1 $2" "$plist" >/dev/null 2>&1 \
        || "$PLISTBUDDY" -c "Add :$1 string $2" "$plist" >/dev/null
}
set_string CFBundleIdentifier "$BUNDLE_ID"
set_string CFBundleName "$BUNDLE_NAME"
set_string CFBundleDisplayName "$BUNDLE_NAME"
set_string CFBundleIconFile "$ICON_NAME"

say "signing"
# --deep so the nested binary is re-signed too; --identifier must match the
# bundle id or macOS treats it as a different app than the one you granted.
codesign --force --deep --sign - --identifier "$BUNDLE_ID" "$APP"
codesign -dv "$APP" 2>&1 | sed 's/^/   /'

say "registering with Launch Services"
if [ -x "$LSREGISTER" ]; then
    "$LSREGISTER" -f "$APP"
    printf '   done\n'
else
    printf '   %s not found, skipped\n' "$LSREGISTER"
fi

say "diagnose"
"$APP/Contents/MacOS/terminal-notifier" -diagnose 2>&1 | sed 's/^/   /'

say "built $APP"
