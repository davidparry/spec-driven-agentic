#!/bin/sh
# Rebuild spec from a clean tree and pin the decision model to enforce.
#
# Every .spec directory loses everything except config.toml, and that
# file's [decision] mode is enforce, so spec deliver and spec greenfield
# do not run advisory. The legacy names the harness migrates back into
# an emptied .spec/ go with them. Then the crate is cleaned, the
# lockfile is re-resolved, the installed package is removed, spec is
# installed again, and what landed on PATH is checked against the
# manifest. Nothing is carried over: no target directory, no compiler
# cache, no previously installed binary.
#
# The version in harness/Cargo.toml is left alone. Move it yourself when
# a build is going out.
#
# Usage, from anywhere:
#   scripts/nuke-build.sh
set -eu
cd "$(dirname "$0")/.."

die() {
    echo "ERROR: $1" >&2
    exit 1
}

command -v cargo >/dev/null 2>&1 || die "cargo is not on PATH"
[ -f harness/Cargo.toml ] || die "no harness/Cargo.toml under $(pwd)"

CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"

crate_version() {
    sed -n 's/^version = "\(.*\)"$/\1/p' harness/Cargo.toml | head -1
}

# An emptied target directory is not a clean build if rustc is behind a
# shared compilation cache, or if the artifacts land somewhere cargo
# clean never reached. Drop every setting that does either, for this run
# only, so the environment the script inherits cannot weaken it.
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER \
    CARGO_BUILD_RUSTC_WRAPPER CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER \
    CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR \
    RUSTFLAGS CARGO_BUILD_RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
export CARGO_INCREMENTAL=0

# The same two settings reach cargo from its config files, where
# unsetting the environment does not touch them. harness/.cargo/config.toml
# is legitimate and sets only RUST_TEST_THREADS, so this matches the keys
# that matter rather than the presence of a file.
for config in "${CARGO_HOME:-$HOME/.cargo}/config.toml" .cargo/config.toml harness/.cargo/config.toml; do
    [ -f "$config" ] || continue
    if grep -Eq '^[[:space:]]*(rustc-wrapper|rustc_wrapper|target-dir|target_dir)[[:space:]]*=' "$config"; then
        die "$config sets a rustc wrapper or a target directory; a shared cache or an uncleaned target would outlive this script"
    fi
done

# The plane-wide [decision] mode, not a per-gate override. An active
# mode line is rewritten. A commented one is what the file uses to show
# the default, so that comment becomes the active enforce line. A
# [decision] table with neither gains the line. A file with no
# [decision] table gains the table.
set_decision_enforce() {
    file="$1"
    [ -f "$file" ] || return 0
    perl -0pi -e '
        my $set = sub {
            my ($section) = @_;
            if ($section =~ /^mode[ \t]*=[ \t]*"[^"]*"/m) {
                $section =~ s/^mode[ \t]*=[ \t]*"[^"]*"/mode = "enforce"/m;
            } elsif ($section =~ /^#[ \t]*mode[ \t]*=[ \t]*"[^"]*"/m) {
                $section =~ s/^#[ \t]*mode[ \t]*=[ \t]*"[^"]*"/mode = "enforce"/m;
            } else {
                $section =~ s/\A(\[[^\]]+\]\n)/$1mode = "enforce"\n/;
            }
            return $section;
        };
        if ($_ =~ /^\[decision\]/m) {
            my @parts = split /(?=^\[)/m, $_;
            my $seen = 0;
            for my $part (@parts) {
                if (!$seen && $part =~ /^\[decision\]/) {
                    $part = $set->($part);
                    $seen = 1;
                }
            }
            $_ = join "", @parts;
        } else {
            $_ .= "\n" unless /\n\z/;
            $_ .= "[decision]\nmode = \"enforce\"\n";
        }
    ' "$file"
    echo "decision mode = enforce in $file"
}

clean_spec_dir() {
    dir="$1"
    [ -d "$dir" ] || return 0
    find "$dir" -mindepth 1 -maxdepth 1 ! -name 'config.toml' -exec rm -rf {} +
    if [ ! -f "$dir/config.toml" ]; then
        printf '[decision]\nmode = "enforce"\n' > "$dir/config.toml"
        echo "wrote $dir/config.toml with decision mode = enforce"
    else
        set_decision_enforce "$dir/config.toml"
    fi
    echo "cleaned $dir (config.toml kept)"
}

# ensure_spec_home() moves each of these from a project root into a
# .spec/ that does not already hold it, so one left behind is state the
# next spec run puts back after clean_spec_dir has just deleted it.
clean_legacy_state() {
    root="$1"
    for name in .spec.toml .spec-state.json .spec-memory.json \
        .spec-history .spec-cache .spec-log .bdd-log; do
        [ -e "$root/$name" ] || continue
        rm -rf "${root:?}/$name"
        echo "removed legacy $root/$name"
    done
}

echo "spec-harness $(crate_version)"

clean_spec_dir ".spec"
clean_spec_dir "harness/.spec"
clean_legacy_state "."
clean_legacy_state "harness"

echo "cargo clean"
(cd harness && cargo clean)
[ ! -d harness/target ] || die "harness/target survived cargo clean"

echo "cargo update"
(cd harness && cargo update)
echo "harness/Cargo.lock was re-resolved; review and commit it, or"
echo "git checkout -- harness/Cargo.lock to go back to the pinned set"

echo "cargo uninstall spec-harness"
if ! cargo uninstall spec-harness; then
    echo "spec-harness was not cargo-installed; continuing to install"
fi

# A binary from scripts/spec-harness-installer.sh is not in cargo's
# registry, so the uninstall above leaves it and cargo install refuses
# to overwrite a file it does not own.
rm -f "$CARGO_BIN/spec"

echo "cargo install --path . --force"
(cd harness && cargo install --path . --force)

# What was built is only what runs if it is also what PATH resolves to.
version=$(crate_version)
found=$(command -v spec || true)
[ -n "$found" ] || die "spec is not on PATH; put $CARGO_BIN on it"
[ "$found" = "$CARGO_BIN/spec" ] || die "spec on PATH is $found, shadowing the $CARGO_BIN/spec just built"
reported=$("$CARGO_BIN/spec" --version | awk '{print $2}')
[ "$reported" = "$version" ] || die "$CARGO_BIN/spec reports $reported, not the $version in harness/Cargo.toml"
echo "installed spec $version at $found"
