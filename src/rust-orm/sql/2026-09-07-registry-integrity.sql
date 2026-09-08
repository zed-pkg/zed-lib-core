-- Forward-only registry integrity upgrade. Apply in the migrator transaction.
-- Historical DDL is immutable. No data is rewritten and no privilege is added.
-- Validation intentionally aborts an upgrade containing incompatible old rows.
-- The two new indexes need an operator-reviewed lock/size budget on live data.

create unique index if not exists zed_projects_id_org_integrity_uq
  on public.zed_projects (id, org_id);
create unique index if not exists zed_versions_id_package_integrity_uq
  on public.zed_package_versions (id, package_id);

do $zed_integrity$
declare
  spec record;
begin
  -- Identifiers and definitions below are static, package-owned migration input.
  -- MATCH SIMPLE preserves the intentionally nullable project/version pointers.
  -- SET NULL names only that pointer: ownership must never be cleared with it.
  for spec in select * from (values
    ('zed_packages', 'zed_packages_project_org_integrity_fk',
     'foreign key (project_id, org_id) references public.zed_projects (id, org_id) on delete set null (project_id)'),
    ('zed_package_licenses', 'zed_licenses_version_package_integrity_fk',
     'foreign key (package_version_id, package_id) references public.zed_package_versions (id, package_id) on delete cascade'),
    ('zed_package_uploads', 'zed_uploads_version_package_integrity_fk',
     'foreign key (package_version_id, package_id) references public.zed_package_versions (id, package_id) on delete set null (package_version_id)'),
    ('zed_package_downloads', 'zed_downloads_version_package_integrity_fk',
     'foreign key (package_version_id, package_id) references public.zed_package_versions (id, package_id) on delete set null (package_version_id)'),
    ('zed_package_licenses', 'zed_package_licenses_spdx_required_chk',
     'check (kind <> ''spdx'' or spdx_id is not null)')
  ) as guards(table_name, constraint_name, definition)
  loop
    if not exists (
      select 1 from pg_catalog.pg_constraint
      where conrelid = pg_catalog.to_regclass('public.' || spec.table_name)
        and conname = spec.constraint_name
    ) then
      execute format('alter table public.%I add constraint %I %s not valid',
                     spec.table_name, spec.constraint_name, spec.definition);
    end if;
    -- Never silently leave existing inconsistent rows outside the guarantee.
    execute format('alter table public.%I validate constraint %I',
                   spec.table_name, spec.constraint_name);
  end loop;
end
$zed_integrity$;
