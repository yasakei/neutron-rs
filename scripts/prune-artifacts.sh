#!/usr/bin/env bash
# Reclaim disk space taken by regenerable build artifacts.
#
# A `cargo test` run over this workspace leaves behind hundreds of megabytes
# that are never read again: one ~30 MB binary per e2e test target and a
# ~110 MB `libntsc_runtime.a` per fingerprint in `target/debug/deps`, a fresh
# `target/debug/incremental` directory per rebuild, ~39 MB native binaries
# under `examples/*/build`, and the temporary directories the e2e tests link
# their binaries into. Left alone the tree reaches tens of gigabytes.
#
# Everything removed here is reproducible by `cargo build`, `ntsc build`, or
# the release workflow. Nothing tracked by git is touched.
#
# Usage:
#   prune-artifacts.sh              prune regenerable artifacts
#   prune-artifacts.sh --dry-run    report what would be removed
#   prune-artifacts.sh --deep       also drop dist/ and all of target/
#   prune-artifacts.sh --quiet      only print the reclaimed total
#   prune-artifacts.sh --keep N     keep the N newest builds per crate (default 1)

set -uo pipefail

readonly REWRITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

dry_run=false
deep=false
quiet=false
keep=1

while [[ $# -gt 0 ]]; do
	case "$1" in
	--dry-run | -n) dry_run=true ;;
	--deep) deep=true ;;
	--quiet | -q) quiet=true ;;
	--keep)
		shift
		keep="${1:-1}"
		;;
	--help | -h)
		sed -n '2,26p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
		exit 0
		;;
	*)
		printf 'prune-artifacts: unknown option `%s`\n' "$1" >&2
		exit 2
		;;
	esac
	shift
done

if ! [[ "$keep" =~ ^[0-9]+$ ]]; then
	printf 'prune-artifacts: --keep expects a number, got `%s`\n' "$keep" >&2
	exit 2
fi

reclaimed_kb=0

log() {
	[[ "$quiet" == true ]] || printf '%s\n' "$*"
}

# ── Concurrency guard ────────────────────────────────────────────────────
#
# Pruning `target/` under a running build corrupts it: rustc writes
# `incremental/<target>/s-*-working/dep-graph.part.bin` and renames it at the
# end of a compile, so removing the directory mid-flight fails the build with
# "failed to move dependency graph ... No such file or directory". Deleting
# `deps/` artifacts a test run has not launched yet breaks it the same way.
#
# Two independent signals, because neither alone covers the whole window:
# cargo holds `target/debug/.cargo-lock` only while compiling, and released it
# by the time the test binaries run.
build_in_progress() {
	local lock="$REWRITE_DIR/target/debug/.cargo-lock"
	if [[ -e "$lock" ]] && ! flock --nonblock --exclusive "$lock" true 2>/dev/null; then
		return 0
	fi
	pgrep -x cargo >/dev/null 2>&1 || pgrep -x rustc >/dev/null 2>&1
}

# Disk usage of a path in KiB, or 0 when it does not exist.
usage_kb() {
	[[ -e "$1" ]] || {
		printf '0'
		return
	}
	du -sk -- "$1" 2>/dev/null | cut -f1
}

# Format KiB for humans without depending on `bc` or `numfmt`.
human() {
	local kb=$1
	if ((kb >= 1048576)); then
		printf '%d.%d GiB' "$((kb / 1048576))" "$(((kb % 1048576) * 10 / 1048576))"
	elif ((kb >= 1024)); then
		printf '%d MiB' "$((kb / 1024))"
	else
		printf '%d KiB' "$kb"
	fi
}

# Remove a file or directory, accounting for the space it freed.
drop() {
	local target=$1 label=${2-}
	[[ -e "$target" ]] || return 0
	local size
	size=$(usage_kb "$target")
	((size > 0)) || size=0
	reclaimed_kb=$((reclaimed_kb + size))
	if [[ "$dry_run" == true ]]; then
		log "  would remove ${label:-$target} ($(human "$size"))"
		return 0
	fi
	rm -rf -- "$target" || {
		printf 'prune-artifacts: failed to remove %s\n' "$target" >&2
		reclaimed_kb=$((reclaimed_kb - size))
		return 0
	}
	log "  removed ${label:-$target} ($(human "$size"))"
}

# ── Stale cargo artifacts ────────────────────────────────────────────────
#
# Cargo never garbage-collects `target/debug/deps`: every recompile of a crate
# writes a new `<name>-<16 hex fingerprint>` artifact and leaves the previous
# ones in place. Grouping by the fingerprint-stripped name and keeping the
# newest `--keep` per group drops the superseded copies while leaving the
# current build intact — cargo rebuilds anything it still needs on demand.
prune_stale_deps() {
	local deps_dir="$REWRITE_DIR/target/debug/deps"
	[[ -d "$deps_dir" ]] || return 0
	log "Stale cargo artifacts in target/debug/deps:"

	local stale_count=0
	while IFS= read -r -d '' path; do
		drop "$path" "deps/$(basename -- "$path")"
		stale_count=$((stale_count + 1))
	done < <(stale_dep_paths "$deps_dir")

	((stale_count > 0)) || log "  nothing stale"
}

# Emit the NUL-separated paths of superseded artifacts, newest-first per group.
stale_dep_paths() {
	local deps_dir=$1
	find "$deps_dir" -maxdepth 1 -mindepth 1 -printf '%T@\t%p\0' 2>/dev/null |
		sort -zrn |
		awk -v keep="$keep" 'BEGIN { RS = "\0"; ORS = "\0"; FS = "\t" }
			{
				path = $2
				name = path
				sub(/.*\//, "", name)
				# Collapse the fingerprint so every rebuild of one crate target
				# shares a group key, e.g. libntsc_runtime-<hash>.a.
				gsub(/-[0-9a-f]{16}/, "-", name)
				if (++seen[name] > keep) print path
			}'
}

# ── Incremental compilation caches ───────────────────────────────────────
#
# `target/debug/incremental` grows a new session directory per rebuild and is
# purely a compile-time speedup; dropping it costs one cold rebuild.
prune_incremental() {
	log "Incremental caches:"
	drop "$REWRITE_DIR/target/debug/incremental" "target/debug/incremental"
	drop "$REWRITE_DIR/target/release/incremental" "target/release/incremental"
}

# ── Compiled NTSC programs ───────────────────────────────────────────────
#
# `ntsc build` statically links the runtime into every example, so each
# `examples/*/build` and `benchmarks/_build` binary is ~20-40 MB. Both trees
# are gitignored and rebuilt by `ntsc build` / the benchmark runner.
prune_ntsc_builds() {
	log "Compiled NTSC programs:"
	local removed=false
	while IFS= read -r -d '' build_dir; do
		drop "$build_dir" "${build_dir#"$REWRITE_DIR"/}"
		removed=true
	done < <(find "$REWRITE_DIR/examples" -mindepth 2 -maxdepth 2 -type d -name build -print0 2>/dev/null)

	if [[ -d "$REWRITE_DIR/benchmarks/_build" ]]; then
		drop "$REWRITE_DIR/benchmarks/_build" "benchmarks/_build"
		removed=true
	fi
	[[ "$removed" == true ]] || log "  nothing to remove"
}

# ── Temporary e2e output ─────────────────────────────────────────────────
#
# The codegen e2e tests link their binaries into `$TMPDIR/ntsc_*`. Each test
# removes its own directory on success, so anything left belongs to a failed or
# interrupted run.
prune_temp_dirs() {
	log "Temporary e2e directories:"
	local tmp="${TMPDIR:-/tmp}" removed=false
	while IFS= read -r -d '' dir; do
		drop "$dir" "$dir"
		removed=true
	done < <(find "$tmp" -maxdepth 1 -mindepth 1 -type d \
		\( -name 'ntsc_*' -o -name 'ntsc-e2e-*' \) -print0 2>/dev/null)
	drop "$REWRITE_DIR/target/tmp" "target/tmp"
	[[ "$removed" == true ]] || log "  nothing to remove"
}

# ── Release packaging output ─────────────────────────────────────────────
#
# `--deep` only: the tarballs and packages under `dist/` are ~170 MB and are
# rebuilt by the release workflow.
prune_dist() {
	log "Release packages:"
	drop "$REWRITE_DIR/dist" "dist"
	drop "$REWRITE_DIR/.github/packaging/.build" ".github/packaging/.build"
}

# ── Whole cargo target directory ─────────────────────────────────────────
#
# `--deep` only: forces a full rebuild including LLVM/inkwell, which takes
# minutes, so this is never part of the default post-test prune.
prune_target() {
	log "Cargo target directory:"
	drop "$REWRITE_DIR/target" "target"
}

before_kb=$(usage_kb "$REWRITE_DIR")

if [[ "$dry_run" == false ]] && build_in_progress; then
	printf 'prune-artifacts: a cargo build is in progress; skipping (tree is %s)\n' \
		"$(human "$before_kb")"
	exit 0
fi

if [[ "$deep" == true ]]; then
	prune_temp_dirs
	prune_ntsc_builds
	prune_dist
	prune_target
else
	prune_stale_deps
	prune_incremental
	prune_ntsc_builds
	prune_temp_dirs
fi

after_kb=$(usage_kb "$REWRITE_DIR")

if [[ "$dry_run" == true ]]; then
	printf 'prune-artifacts: would reclaim %s (tree is %s)\n' \
		"$(human "$reclaimed_kb")" "$(human "$before_kb")"
else
	printf 'prune-artifacts: reclaimed %s (tree %s → %s)\n' \
		"$(human "$reclaimed_kb")" "$(human "$before_kb")" "$(human "$after_kb")"
fi
