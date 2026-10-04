#!/bin/sh
# btrfs with directories: a snapshot is a copy, and what btrfs keeps inside a subvolume (its UUID,
# the UUID it was received from, read-only) lives in a hidden file beside it. A filesystem starts
# wherever a .fake-btrfs-filesystem file holds its UUID. A receive into a directory holding a
# .fake-btrfs-fail-receive file stops half way.
set -eu

die() {
    echo "ERROR: $*" >&2
    exit 1
}

meta() { printf '%s/.%s.fake-btrfs' "$(dirname "$1")" "$(basename "$1")"; }

field() {
    if [ -f "$(meta "$1")" ]; then sed -n "s/^$2=//p" "$(meta "$1")"; fi
}

write_meta() { printf 'uuid=%s\nreceived=%s\nread_only=%s\n' "$2" "$3" "$4" >"$(meta "$1")"; }

new_uuid() { cat /proc/sys/kernel/random/uuid 2>/dev/null || uuidgen | tr 'A-F' 'a-f'; }

filesystem() {
    dir=$(cd "$1" && pwd -P) || return 1
    while :; do
        if [ -f "$dir/.fake-btrfs-filesystem" ]; then
            cat "$dir/.fake-btrfs-filesystem"
            return
        fi
        [ "$dir" != / ] || die "$1 is not on a btrfs filesystem"
        dir=$(dirname "$dir")
    done
}

snapshot() {
    read_only=$1 source=$2 target=$3
    [ -d "$source" ] || die "cannot snapshot '$source': not a subvolume"
    [ ! -e "$target" ] || die "target path already exists: $target"
    [ "$(filesystem "$source")" = "$(filesystem "$(dirname "$target")")" ] ||
        die "cannot snapshot '$source' into '$(dirname "$target")': Invalid cross-device link"
    cp -Rp "$source" "$target"
    write_meta "$target" "$(new_uuid)" - "$read_only"
    if [ "$read_only" = true ]; then
        echo "Create a readonly snapshot of '$source' in '$target'"
    else
        echo "Create a snapshot of '$source' in '$target'"
    fi
}

show() {
    [ -d "$1" ] || die "cannot find subvolume $1"
    uuid=$(field "$1" uuid)
    # the live subvolume was never snapshotted into being, so it has no meta file: a UUID that stays put
    [ -n "$uuid" ] || uuid=$(printf %s "$1" | cksum | cut -d' ' -f1)
    flags=-
    [ "$(field "$1" read_only)" != true ] || flags=readonly
    received=$(field "$1" received)
    printf '%s\n\tName: \t\t\t%s\n\tUUID: \t\t\t%s\n\tReceived UUID: \t\t%s\n\tFlags: \t\t\t%s\n' \
        "$1" "$(basename "$1")" "$uuid" "${received:--}" "$flags"
}

# the stream is one header line, then a tar of the snapshot
send() {
    parent=-
    if [ "$1" = -p ]; then
        [ -d "$2" ] || die "cannot open parent $2"
        parent=$(field "$2" uuid)
        shift 2
    fi
    [ -d "$1" ] || die "cannot open $1"
    [ "$(field "$1" read_only)" = true ] || die "$1 is not read-only"
    echo "At subvol $1" >&2
    echo "fake-btrfs-stream $(field "$1" uuid) $parent $(basename "$1")"
    tar -cf - -C "$(dirname "$1")" "$(basename "$1")"
}

receive() {
    destination=$1
    read -r magic uuid parent name
    [ "$magic" = fake-btrfs-stream ] || die "not a btrfs send stream"
    if [ "$parent" != - ]; then
        found=
        for candidate in "$destination"/*; do
            if [ -d "$candidate" ] && [ "$(field "$candidate" received)" = "$parent" ]; then found=$candidate; fi
        done
        [ -n "$found" ] || {
            cat >/dev/null
            die "cannot find parent subvolume"
        }
    fi
    target=$destination/$name
    [ ! -e "$target" ] || {
        cat >/dev/null
        die "$target already exists"
    }
    if [ -e "$destination/.fake-btrfs-fail-receive" ]; then
        mkdir "$target"
        write_meta "$target" "$(new_uuid)" - false
        cat >/dev/null
        die "receive was interrupted"
    fi
    tar -xf - -C "$destination"
    cat >/dev/null
    write_meta "$target" "$(new_uuid)" "$uuid" true
    echo "At snapshot $name" >&2
}

case "$*" in
"subvolume snapshot -r "*) shift 3 && snapshot true "$@" ;;
"subvolume snapshot "*) shift 2 && snapshot false "$@" ;;
"subvolume show "*) shift 2 && show "$1" ;;
"subvolume delete "*)
    shift 2
    for path in "$@"; do
        [ -d "$path" ] || die "cannot delete '$path': not a subvolume"
        rm -rf "$path" "$(meta "$path")"
        echo "Delete subvolume (no-commit): '$path'"
    done
    ;;
"send "*) shift && send "$@" ;;
"receive "*) shift && receive "$1" ;;
*) die "the fake btrfs cannot: btrfs $*" ;;
esac
