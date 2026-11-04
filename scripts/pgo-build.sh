#!/usr/bin/env bash
set -euo pipefail

ext=""
native() { printf '%s' "$1"; }
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
        ext=".exe"
        native() { cygpath -w "$1"; }
        ;;
esac

if [ "$#" -eq 0 ]; then
    echo "usage: $0 <a tree to count> [more trees]" >&2
    echo "The repository and its language fixtures are counted anyway, but they are four" >&2
    echo "milliseconds of work, and a profile that small decides nothing: two builds from" >&2
    echo "it landed five percent apart. Name a real corpus." >&2
    exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
rm -rf target-pgo pgo-data pgo.profdata

CARGO_TARGET_DIR=target-pgo RUSTFLAGS="-Cprofile-generate=$(native "$root/pgo-data")" \
        cargo build --release -p mezura

counter="target-pgo/release/mezura$ext"
for pass in 1 2 3; do
    "$counter" . --threads 1 1 > /dev/null
    "$counter" mezura-core/tests/fixtures --threads 1 1 > /dev/null
    "$counter" . --counting region --hide keywords --threads 1 1 > /dev/null
    for target in "$@"; do
        "$counter" "$target" --threads 1 1 > /dev/null
    done
done

sysroot="$(rustc --print sysroot)"
case "$ext" in .exe) sysroot="$(cygpath -u "$sysroot")" ;; esac
profdata="$(find "$sysroot" -name "llvm-profdata$ext" -type f | head -1)"
if [ -z "$profdata" ]; then
    echo "llvm-profdata is not installed: rustup component add llvm-tools" >&2
    exit 1
fi
"$profdata" merge -o "$root/pgo.profdata" pgo-data

RUSTFLAGS="-Cprofile-use=$(native "$root/pgo.profdata") -Cllvm-args=-pgo-warn-missing-function" \
        cargo build --release -p mezura 2>&1 | tee target-pgo/profile-use.log

unmatched="$(grep 'no profile data available for function' target-pgo/profile-use.log | grep -vc build_script_build || true)"
if [ "$unmatched" -gt 0 ]; then
    echo "The profile matches none of $unmatched functions of this build, so the binary would ship" >&2
    echo "without PGO. The two builds above compiled the crates differently: other packages, other" >&2
    echo "features or other cargo profile settings change the names the profile is keyed by." >&2
    exit 1
fi

rm -rf target-pgo pgo-data
