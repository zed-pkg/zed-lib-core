#!/bin/sh
set -eu

schema_root=${ZED_PKG_TEST_TARGET:?ZED_PKG_TEST_TARGET is required}

test -f "$schema_root/registry.sql"
test -f "$schema_root/2026-08-11-dependency-graph-artifacts.sql"
test -f "$schema_root/2026-08-11-public-visibility-is-permanent.sql"

for sqlstate in ZD001 ZD002 ZD003 ZD004 ZD005; do
  grep -q "errcode = '$sqlstate'" "$schema_root/registry.sql"
done
