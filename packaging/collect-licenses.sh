#!/bin/sh
# Preserve upstream license/notice files for every fetched Cargo dependency.
set -eu
destination=$1
registry_root=${CARGO_HOME:-/usr/local/cargo}/registry/src
mkdir -p "$destination"
cargo metadata --locked --format-version 1 >"$destination/cargo-metadata.json"
find "$registry_root" -type f \( -iname '*license*' -o -iname '*copying*' -o -iname '*notice*' \) -exec sh -c '
    destination=$1
    registry_root=$2
    shift 2
    for source_path do
        relative_path=${source_path#"$registry_root"/}
        target_path="$destination/$relative_path"
        mkdir -p "${target_path%/*}"
        cp "$source_path" "$target_path"
    done
' sh "$destination" "$registry_root" {} +

# Bundle the actual compiled MPL-covered source, including any local changes.
# Fail the build on a dependency upgrade until its source notice is reviewed.
cargo tree --locked --edges normal -i ece@2.3.1 >/dev/null
set -- "$registry_root"/*/ece-2.3.1
[ "$#" -eq 1 ] && [ -f "$1/Cargo.toml" ] || {
    echo 'Expected exactly one ece 2.3.1 source tree' >&2
    exit 1
}
mkdir -p "$destination/sources"
tar -czf "$destination/sources/ece-2.3.1.tar.gz" -C "${1%/*}" "${1##*/}"
(cd "$destination/sources" && sha256sum ece-2.3.1.tar.gz > SHA256SUMS)
