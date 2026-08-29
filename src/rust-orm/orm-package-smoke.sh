#!/bin/sh
set -eu

orm_root=${ZED_PKG_TEST_TARGET:?ZED_PKG_TEST_TARGET is required}

test -f "$orm_root/Cargo.toml"
test -f "$orm_root/lib.rs"
test -f "$orm_root/sql/registry.sql"

grep -q 'default = \["read-only"\]' "$orm_root/Cargo.toml"
grep -q 'read-write = \["read-only"\]' "$orm_root/Cargo.toml"
grep -q 'migrate = \["read-write"\]' "$orm_root/Cargo.toml"

cargo metadata --format-version 1 --no-deps --manifest-path "$orm_root/Cargo.toml" >/dev/null
