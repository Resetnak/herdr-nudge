#!/bin/sh
# Builds bin/herdr-nudge: one universal binary, x86_64 and arm64, which the
# manifest runs on either kind of Mac.
#
# bin/herdr-nudge is committed. `herdr plugin install` is a shallow git
# fetch, and the only other way to get a binary to a user is a build step at
# install time, which would need either their own Rust toolchain or a
# download. So run this before committing a change to src/, and commit the
# binary with it. A linked checkout runs bin/ too, so a change to src/ isn't
# live in Herdr until this has run.
#
# It also writes bin/herdr-nudge.inputs, a hash of what the binary was built
# from, so `--check` can tell a binary older than its source. A hash rather
# than git history, because an edit that doesn't change the output (a
# comment, a test) rebuilds to the same bytes and leaves git nothing to
# commit for the binary itself. CI runs `--check`, but only after the push,
# and main is what `herdr plugin install` fetches, so run it before pushing.
#
#   tools/build-bin.sh           build, and write both files
#   tools/build-bin.sh --check   exit 1 if the source changed since the build

set -eu

die() { printf 'build-bin.sh: %s\n' "$*" >&2; exit 1; }

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# What git would commit: tracked files plus new ones not ignored, so a file
# not added yet counts here as it will in CI's checkout. A tracked file
# deleted from disk is left out (shasum's complaint is dropped), as it will
# be once the deletion is committed. File names are hashed too, so a rename
# counts. Sorted, because ls-files lists tracked files before new ones, and
# committing a new file would otherwise change the hash. The list is checked
# on its own first: sh has no pipefail, so a git failure mid-pipe would hash
# an empty list and pass for a real answer.
inputs_hash() {
    list=$(mktemp "${TMPDIR:-/tmp}/herdr-nudge-inputs.XXXXXX")
    if ! git ls-files -z --cached --others --exclude-standard -- \
        src shell Cargo.toml Cargo.lock tools/build-bin.sh \
        build.rs .cargo rust-toolchain rust-toolchain.toml >"$list"; then
        rm -f "$list"
        die "git ls-files failed; this needs a git checkout"
    fi
    [ -s "$list" ] || { rm -f "$list"; die "git found none of the source files"; }
    LC_ALL=C sort -z <"$list" \
        | xargs -0 shasum -a 256 2>/dev/null | shasum -a 256 | awk '{ print $1 }'
    rm -f "$list"
}

case "${1-}" in
    --check)
        [ -f bin/herdr-nudge.inputs ] || die "no bin/herdr-nudge.inputs; run tools/build-bin.sh"
        now=$(inputs_hash)
        [ "$now" = "$(cat bin/herdr-nudge.inputs)" ] \
            || die "the source changed since bin/herdr-nudge was built; run tools/build-bin.sh and commit bin/"
        echo "bin/herdr-nudge matches the source"
        exit 0
        ;;
    "") ;;
    *) die "unknown argument: $1" ;;
esac

[ "$(uname -s)" = Darwin ] || die "macOS only"
for tool in cargo rustup rustc lipo codesign strings git shasum; do
    command -v "$tool" >/dev/null 2>&1 || die "$tool not found"
done

targets="x86_64-apple-darwin aarch64-apple-darwin"
installed=$(rustup target list --installed)
for t in $targets; do
    printf '%s\n' "$installed" | grep -qx "$t" \
        || die "missing target $t. Run: rustup target add $t"
done

# Panic messages carry the source path of the crate they come from: under
# ~/.cargo for a dependency, and under the toolchain for std once rust-src is
# installed. Without this the committed binary would name whoever built it.
# CARGO_ENCODED_RUSTFLAGS is split on 0x1f, not spaces, so a path with a
# space in it stays one flag.
cargo_home=${CARGO_HOME:-$HOME/.cargo}
sysroot=$(rustc --print sysroot)
sep=$(printf '\037')
CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$cargo_home=/cargo$sep--remap-path-prefix=$sysroot=/rust$sep--remap-path-prefix=$root=."
export CARGO_ENCODED_RUSTFLAGS

# Pinned so a MACOSX_DEPLOYMENT_TARGET in the builder's shell can't raise the
# oldest macOS the committed binary runs on. These are rustc's own defaults.
set --
for t in $targets; do
    case "$t" in
        x86_64-*) min=10.12 ;;
        aarch64-*) min=11.0 ;;
    esac
    MACOSX_DEPLOYMENT_TARGET=$min cargo build --release --locked --target "$t"
    set -- "$@" "target/$t/release/herdr-nudge"
done

mkdir -p bin
lipo -create -output bin/herdr-nudge.tmp "$@"
# The linker signs the arm64 half already, and Apple Silicon won't run it
# without that. Sign the whole file so both halves are signed the same way.
codesign --force --sign - bin/herdr-nudge.tmp

if strings -a bin/herdr-nudge.tmp | grep -F -e "$HOME" -e "$root" >/dev/null; then
    rm -f bin/herdr-nudge.tmp
    die "the binary still contains $HOME or $root; not written"
fi
mv bin/herdr-nudge.tmp bin/herdr-nudge
inputs_hash > bin/herdr-nudge.inputs

lipo -info bin/herdr-nudge
ls -l bin/herdr-nudge
bin/herdr-nudge --version
