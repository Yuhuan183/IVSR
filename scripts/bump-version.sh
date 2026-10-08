#!/bin/sh
# Sets the release version everywhere it is declared: the Cargo workspace
# (every crate, the CLI and the desktop app, which Tauri reads) and the
# desktop package.json. Commit the result, then tag `v<version>` and push the
# tag to start the release workflow.
set -eu

version=${1:-}
if ! printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
    echo "usage: scripts/bump-version.sh <version>   e.g. 0.2.0 or 0.2.0-beta.1" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/.." && pwd)

# The only line-leading `version =` in the root manifest is [workspace.package].
perl -0pi -e "s/^version = \"[^\"]*\"/version = \"$version\"/m" "$root/Cargo.toml"
perl -pi -e "s/^  \"version\": \"[^\"]*\"/  \"version\": \"$version\"/" "$root/apps/desktop/package.json"
cargo update --workspace --manifest-path "$root/Cargo.toml" --quiet

echo "version set to $version; commit it, then tag v$version and push the tag"
