\set ON_ERROR_STOP on
begin;
select set_config('zed.test_hardened', :'hardened', true);
\ir fixtures.sql

-- On the historical schema, prove each bad write actually succeeds. After the
-- upgrade, require its exact integrity error. Every attempt uses a subtransaction
-- so even an accepted baseline write cannot contaminate the next fixture.
create function pg_temp.assert_integrity_guard(
  label text, command text, expected_state text, expected_constraint text
) returns void language plpgsql as $$
declare
  observed_state text;
  observed_constraint text;
  hardened boolean := current_setting('zed.test_hardened')::boolean;
begin
  begin
    execute command;
    raise exception using errcode = 'ZT001', message = 'roll back accepted test write';
  exception when others then
    get stacked diagnostics observed_state = returned_sqlstate,
                            observed_constraint = constraint_name;
  end;
  if hardened then
    if observed_state <> expected_state
       or (expected_constraint is not null and observed_constraint <> expected_constraint) then
      raise exception 'guard %: expected % / %, observed % / %',
        label, expected_state, expected_constraint, observed_state, observed_constraint;
    end if;
    raise notice 'HARDENED: % rejected by %', label, observed_constraint;
  else
    if observed_state <> 'ZT001' then
      raise exception 'baseline % did not reproduce: % / %', label, observed_state, observed_constraint;
    end if;
    raise notice 'BASELINE DEFECT REPRODUCED: % was accepted', label;
  end if;
end;
$$;

select pg_temp.assert_integrity_guard('SPDX missing identifier on insert',
  $q$insert into public.zed_package_licenses (package_id, kind, is_primary)
     values (md5('integrity-package-a')::uuid, 'spdx', false)$q$,
  '23514', 'zed_package_licenses_spdx_required_chk');
select pg_temp.assert_integrity_guard('SPDX identifier removed on update',
  $q$update public.zed_package_licenses set spdx_id = null
     where id = md5('integrity-license')::uuid$q$,
  '23514', 'zed_package_licenses_spdx_required_chk');
select pg_temp.assert_integrity_guard('cross-org project on insert',
  $q$insert into public.zed_packages (org_id, project_id, name)
     values (md5('integrity-org-a')::uuid, md5('integrity-project-b')::uuid, 'cross-org')$q$,
  '23503', 'zed_packages_project_org_integrity_fk');
select pg_temp.assert_integrity_guard('cross-org project on update',
  $q$update public.zed_packages set project_id = md5('integrity-project-b')::uuid
     where id = md5('integrity-package-a')::uuid$q$,
  '23503', 'zed_packages_project_org_integrity_fk');
select pg_temp.assert_integrity_guard('license foreign version on insert',
  $q$insert into public.zed_package_licenses (package_id, package_version_id, kind, is_primary)
     values (md5('integrity-package-a')::uuid, md5('integrity-version-b')::uuid, 'proprietary', false)$q$,
  '23503', 'zed_licenses_version_package_integrity_fk');
select pg_temp.assert_integrity_guard('license foreign version on update',
  $q$update public.zed_package_licenses set package_version_id = md5('integrity-version-b')::uuid
     where id = md5('integrity-license')::uuid$q$,
  '23503', 'zed_licenses_version_package_integrity_fk');
select pg_temp.assert_integrity_guard('upload foreign version on insert',
  $q$insert into public.zed_package_uploads (package_id, package_version_id, requested_version)
     values (md5('integrity-package-a')::uuid, md5('integrity-version-b')::uuid, '1.0.0')$q$,
  '23503', 'zed_uploads_version_package_integrity_fk');
select pg_temp.assert_integrity_guard('upload foreign version on update',
  $q$update public.zed_package_uploads set package_version_id = md5('integrity-version-b')::uuid
     where id = md5('integrity-upload')::uuid$q$,
  '23503', 'zed_uploads_version_package_integrity_fk');
select pg_temp.assert_integrity_guard('download foreign version on insert',
  $q$insert into public.zed_package_downloads (package_id, package_version_id)
     values (md5('integrity-package-a')::uuid, md5('integrity-version-b')::uuid)$q$,
  '23503', 'zed_downloads_version_package_integrity_fk');
select pg_temp.assert_integrity_guard('download foreign version on update',
  $q$update public.zed_package_downloads set package_version_id = md5('integrity-version-b')::uuid
     where id = md5('integrity-download')::uuid$q$,
  '23503', 'zed_downloads_version_package_integrity_fk');
select pg_temp.assert_integrity_guard('referenced project changes org',
  $q$update public.zed_projects set org_id = md5('integrity-org-b')::uuid
     where id = md5('integrity-project-a')::uuid$q$,
  '23503', 'zed_packages_project_org_integrity_fk');
-- Any one of the three new dependent FKs may detect this parent update first.
select pg_temp.assert_integrity_guard('referenced version changes package',
  $q$update public.zed_package_versions set package_id = md5('integrity-package-b')::uuid, version = '2.0.0'
     where id = md5('integrity-version-a')::uuid$q$,
  '23503', null);

do $$
begin
  if (select download_count from public.zed_packages where id = md5('integrity-package-a')::uuid) <> 1
     or (select download_count from public.zed_packages where id = md5('integrity-package-b')::uuid) <> 0
     or (select download_count from public.zed_package_versions where id = md5('integrity-version-a')::uuid) <> 1
     or (select download_count from public.zed_package_versions where id = md5('integrity-version-b')::uuid) <> 0
     or (select count(*) from public.zed_package_downloads where id = md5('integrity-download')::uuid) <> 1 then
    raise exception 'failed writes contaminated download accounting';
  end if;
end;
$$;

-- Nullable bindings remain legal: org-owned packages and package-level metadata.
insert into public.zed_packages (org_id, name)
values (md5('integrity-org-a')::uuid, 'org-owned');
insert into public.zed_package_licenses (package_id, kind, spdx_id)
values (md5('integrity-package-a')::uuid, 'spdx', 'MIT');
insert into public.zed_package_licenses (package_id, kind, is_primary)
values (md5('integrity-package-a')::uuid, 'proprietary', false);
insert into public.zed_package_uploads (package_id, requested_version)
values (md5('integrity-package-a')::uuid, '2.0.0');

-- A verified upload intentionally pins its version under the OLD verified check.
-- Remove that audit fixture before exercising supported version deletion.
delete from public.zed_package_uploads where id = md5('integrity-upload')::uuid;
delete from public.zed_package_versions where id = md5('integrity-version-a')::uuid;
delete from public.zed_projects where id = md5('integrity-project-a')::uuid;
do $$
begin
  if exists (select 1 from public.zed_package_licenses where id = md5('integrity-license')::uuid) then
    raise exception 'version license did not cascade';
  end if;
  if not exists (select 1 from public.zed_package_downloads
     where id = md5('integrity-download')::uuid and package_version_id is null
       and package_id = md5('integrity-package-a')::uuid) then
    raise exception 'version deletion did not retain download ownership';
  end if;
  if not exists (select 1 from public.zed_packages
     where id = md5('integrity-package-a')::uuid and project_id is null
       and org_id = md5('integrity-org-a')::uuid and download_count = 1) then
    raise exception 'project deletion cleared package ownership or lifetime count';
  end if;
end;
$$;
delete from public.zed_packages where id = md5('integrity-package-a')::uuid;
do $$
begin
  if exists (select 1 from public.zed_package_downloads where id = md5('integrity-download')::uuid) then
    raise exception 'package deletion did not retain the existing ledger cascade';
  end if;
end;
$$;
rollback;
