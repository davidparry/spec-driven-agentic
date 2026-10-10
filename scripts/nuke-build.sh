#!/bin/sh
# Rebuild spec from a clean tree and pin the decision model to enforce.
#
# First the patch number in harness/Cargo.toml moves (0.7.10 -> 0.7.11).
# Then every .spec directory loses everything except config.toml, and
# that file's [decision] mode is enforce, so spec deliver and spec
# greenfield do not run advisory. Then the crate is cleaned, the
# installed package is removed, and spec is installed again.
#
# Usage, from anywhere:
#   scripts/nuke-build.sh
set -eu
cd "$(dirname "$0")/.."

bump_patch() {
    manifest="harness/Cargo.toml"
    current=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$manifest" | head -1)
    next=$(printf '%s\n' "$current" | awk -F. '{printf "%d.%d.%d", $1, $2, $3 + 1}')
    perl -pi -e "s/^version = \"\Q$current\E\"\$/version = \"$next\"/" "$manifest"
    echo "spec-harness $current -> $next"
}

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

bump_patch

clean_spec_dir ".spec"
clean_spec_dir "harness/.spec"

echo "cargo clean"
(cd harness && cargo clean)

echo "cargo uninstall spec-harness"
if ! cargo uninstall spec-harness; then
    echo "spec-harness was not installed; continuing to install"
fi

echo "cargo install --path ."
(cd harness && cargo install --path .)
echo "installed spec"
