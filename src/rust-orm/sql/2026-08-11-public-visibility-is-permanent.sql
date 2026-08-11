-- Forward-only compatibility migration for databases that already recorded
-- the original zed-pkg registry segment. The visibility trigger already calls
-- this function, so replacing it upgrades existing databases without replaying
-- non-idempotent CREATE TRIGGER or ADD CONSTRAINT statements.
--
-- Canonical desired state remains in ../registry.sql.
create or replace function zed_enforce_package_visibility_transition()
returns trigger
language plpgsql
set search_path = pg_catalog, public
as $$
declare
  age_days numeric;
  max_age integer := zed_public_conversion_max_age_days();
  max_downloads bigint := zed_public_conversion_max_downloads();
begin
  -- Exact public package artifacts and dependency graphs can be held by shared
  -- caches indefinitely. Once public, the bytes cannot be made confidential.
  if old.visibility = 'public' and new.visibility <> 'public' then
    raise exception
      'public package % cannot become non-public', old.id
      using errcode = 'ZD003';
  end if;

  if old.visibility = 'public' or new.visibility <> 'public' then
    return new;
  end if;

  age_days := extract(epoch from (now() - old.created_at)) / 86400.0;

  if age_days > max_age then
    raise exception
      'package % cannot be made public: it has existed for % days, over the %-day limit',
      old.id, round(age_days, 2), max_age
      using errcode = 'ZD001';
  end if;

  if old.download_count > max_downloads then
    raise exception
      'package % cannot be made public: it has % downloads, over the limit of %',
      old.id, old.download_count, max_downloads
      using errcode = 'ZD002';
  end if;

  new.visibility_changed_at := now();
  return new;
end;
$$;
