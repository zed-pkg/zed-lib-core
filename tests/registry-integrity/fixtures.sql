-- Deterministic synthetic fixtures; each caller owns a transaction and rolls back.
insert into public.zed_orgs (id, slug, name) values
  (md5('integrity-org-a')::uuid, 'integrity-alpha', 'Integrity Alpha'),
  (md5('integrity-org-b')::uuid, 'integrity-beta', 'Integrity Beta');
insert into public.zed_projects (id, org_id, slug, name)
select md5('integrity-project-' || key)::uuid, md5('integrity-org-' || key)::uuid,
       'project-' || key, 'Project ' || key from (values ('a'), ('b')) as f(key);
insert into public.zed_packages (id, org_id, project_id, name)
select md5('integrity-package-' || key)::uuid, md5('integrity-org-' || key)::uuid,
       md5('integrity-project-' || key)::uuid, 'package-' || key
from (values ('a'), ('b')) as f(key);
insert into public.zed_package_versions
  (id, package_id, version, sha256, size_bytes, format, artifact_key)
select md5('integrity-version-' || key)::uuid, md5('integrity-package-' || key)::uuid,
       '1.0.0', repeat('a', 64), 1, 'zip', 'fixture/version-' || key || '.zip'
from (values ('a'), ('b')) as f(key);
insert into public.zed_package_licenses (id, package_id, package_version_id, kind, spdx_id)
values (md5('integrity-license')::uuid, md5('integrity-package-a')::uuid,
        md5('integrity-version-a')::uuid, 'spdx', 'MIT');
insert into public.zed_package_uploads
  (id, package_id, package_version_id, requested_version, status, completed_at,
   storage_backend, storage_key, format, size_bytes, sha256)
values (md5('integrity-upload')::uuid, md5('integrity-package-a')::uuid,
        md5('integrity-version-a')::uuid, '1.0.0', 'verified', clock_timestamp(),
        'r2', 'fixture/version-a.zip', 'zip', 1, repeat('a', 64));
insert into public.zed_package_downloads
  (id, package_id, package_version_id, format, bytes_sent)
values (md5('integrity-download')::uuid, md5('integrity-package-a')::uuid,
        md5('integrity-version-a')::uuid, 'zip', 1);
