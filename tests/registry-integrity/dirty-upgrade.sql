\set ON_ERROR_STOP on
-- Caller MUST use psql --single-transaction. A failed upgrade must undo both
-- its DDL and these synthetic dirty fixtures, without repairing/deleting data.
\ir fixtures.sql
\if :spdx
insert into public.zed_package_licenses (package_id, kind, is_primary)
values (md5('integrity-package-a')::uuid, 'spdx', false);
\else
update public.zed_packages set project_id = md5('integrity-project-b')::uuid
where id = md5('integrity-package-a')::uuid;
\endif
\ir ../../src/rust-orm/sql/2026-09-07-registry-integrity.sql
