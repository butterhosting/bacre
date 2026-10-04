#!/bin/sh
# findmnt --noheadings --output UUID --target <path>, for the fake btrfs: the UUID in the nearest
# .fake-btrfs-filesystem at or above <path>
set -eu

[ "$#" -eq 5 ] && [ "$1 $2 $3 $4" = "--noheadings --output UUID --target" ] || {
    echo "the fake findmnt cannot: findmnt $*" >&2
    exit 1
}
dir=$(cd "$5" && pwd -P)
while [ ! -f "$dir/.fake-btrfs-filesystem" ]; do
    [ "$dir" != / ] || exit 1
    dir=$(dirname "$dir")
done
cat "$dir/.fake-btrfs-filesystem"
