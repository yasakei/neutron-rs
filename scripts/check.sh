#!/usr/bin/env sh
# Batched verification for the ntsc workspace (rewrite/).
#
# The full gate (workspace clippy + workspace tests) is
# heavy and resource-intensive, so it must run only once, right before
# finishing. While iterating, use the targeted targets below.
#
# Usage (from the repo root):
#   scripts/check.sh fmt              cargo fmt (writes)
#   scripts/check.sh lint             clippy -D warnings: ntsc-codegen + ntsc-runtime
#   scripts/check.sh test codegen     ntsc-codegen unit tests (fast, no e2e)
#   scripts/check.sh test runtime     ntsc-runtime unit tests
#   scripts/check.sh e2e [filter]     ntsc-codegen e2e suites matching [filter]
#                                     (e.g. `e2e shared`, `e2e class_array`);
#                                     without a filter it runs every suite
#   scripts/check.sh proj <dir>       build ntsc-cli, then ntsc build + run a
#                                     project (example or scratch dir)
#   scripts/check.sh quick            fmt + lint + unit tests (routine batch)
#   scripts/check.sh full             the complete gate: fmt, workspace clippy,
#                                     workspace tests (once, before finishing)

set -e

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

say() { printf '\n== %s ==\n' "$*"; }

# Cargo build parallelism + test-thread cap, so the gate stays light on CPU
# and memory (a browser or IDE can stay open alongside it). Everything runs
# `nice`d so cargo never starves the desktop. Override with CHECK_JOBS=<n>.
JOBS="${CHECK_JOBS:-2}"
[ "$JOBS" -lt 1 ] && JOBS=1
export RUST_TEST_THREADS="$JOBS"
NICE="$(command -v nice >/dev/null && echo nice -n 19 || echo '')"

# Run cargo at low CPU priority with capped parallelism. -j follows the
# subcommand: it is a per-command flag, not a global one.
cg() {
    cmd="$1"
    shift
    $NICE cargo "$cmd" -j "$JOBS" "$@"
}

CMD="${1:-help}"
shift || true

LINT_TARGETS="clippy -p ntsc-codegen -p ntsc-runtime --all-targets --all-features -- -D warnings"

case "$CMD" in
fmt)
    say "cargo fmt"
    $NICE cargo fmt
    ;;
lint)
    say "clippy -D warnings (ntsc-codegen + ntsc-runtime)"
    cg $LINT_TARGETS
    ;;
test)
    case "${1:-}" in
    codegen)
        say "ntsc-codegen unit tests"
        cg test -p ntsc-codegen --lib
        ;;
    runtime)
        say "ntsc-runtime unit tests"
        cg test -p ntsc-runtime
        ;;
    *)
        echo "usage: scripts/check.sh test <codegen|runtime>" >&2
        exit 2
        ;;
    esac
    ;;
e2e)
    filter="${1:-}"
    e2e_dir="$ROOT/crates/ntsc-codegen/tests"
    if [ -n "$filter" ]; then
        suites="$(
            ls "$e2e_dir"/*_e2e.rs 2>/dev/null |
                sed -E 's#.*/([^/]+)\.rs#\1#' |
                grep -i -- "$filter" || true
        )"
        if [ -z "$suites" ]; then
            echo "no e2e suites match '$filter'" >&2
            exit 2
        fi
    else
        suites="$(ls "$e2e_dir"/*_e2e.rs 2>/dev/null | sed -E 's#.*/([^/]+)\.rs#\1#')"
    fi
    for suite in $suites; do
        say "e2e suite: $suite"
        cg test -p ntsc-codegen --test "$suite"
    done
    ;;
proj)
    dir="${1:-}"
    if [ -z "$dir" ]; then
        echo "usage: scripts/check.sh proj <dir>" >&2
        exit 2
    fi
    say "building ntsc-cli"
    cg build -p ntsc-cli
    say "building and running $dir"
    (
        cd "$dir"
        "$ROOT/target/debug/ntsc" build
        exec ./build/debug/"$(basename "$dir")"
    )
    ;;
quick)
    say "cargo fmt"
    $NICE cargo fmt
    say "clippy -D warnings (ntsc-codegen + ntsc-runtime)"
    cg $LINT_TARGETS
    say "ntsc-runtime unit tests"
    cg test -p ntsc-runtime
    say "ntsc-codegen unit tests"
    cg test -p ntsc-codegen --lib
    ;;
full)
    say "cargo fmt"
    $NICE cargo fmt
    say "clippy -D warnings (workspace)"
    cg clippy --workspace --all-targets --all-features -- -D warnings
    say "cg test --workspace"
    cg test --workspace
    ;;
help)
    sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
    ;;
*)
    echo "unknown command: $CMD (see scripts/check.sh help)" >&2
    exit 2
    ;;
esac