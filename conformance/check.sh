#!/usr/bin/env sh
set -eu

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)"
cd "$repo_root"

[ -d contracts ] && [ ! -L contracts ] || {
  echo 'conformance: contracts/ must be a real non-symlink directory' >&2
  exit 1
}
[ -d conformance ] && [ ! -L conformance ] || {
  echo 'conformance: conformance/ must be a real non-symlink directory' >&2
  exit 1
}

# zed-lib's Rust implementation is the established behavioral oracle for this
# package: Dart and TypeScript independently replay the same committed corpus in
# their own CI lanes. Keep this lifecycle entrypoint focused on the shared
# corpus consumer instead of re-running every unrelated workspace test.
cargo test --locked -p zed-lib --test conformance
