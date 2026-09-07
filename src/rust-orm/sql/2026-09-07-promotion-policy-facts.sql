-- Forward-only upgrade. Historical SQL and ledger identities stay unchanged.
-- The ordered migrator applies this after the public-visibility permanence fix.
-- There are no data rewrites, new privileges, or SECURITY DEFINER functions.

-- Explicit evaluation time makes the inclusive boundary independently testable.
-- The trigger below, not its caller, supplies the real decision timestamp.
create or replace function public.zed_assert_public_conversion(
  package_created_at timestamptz,
  evaluated_at timestamptz,
  completed_downloads bigint
)
returns void
language plpgsql
set search_path = pg_catalog, public
as $$
declare
  max_age integer := public.zed_public_conversion_max_age_days();
  max_downloads bigint := public.zed_public_conversion_max_downloads();
begin
  if isfinite(package_created_at) is not true
     or isfinite(evaluated_at) is not true
     or package_created_at > evaluated_at then
    raise exception 'package creation time must be finite and not in the future'
      using errcode = '23514';
  end if;
  if completed_downloads is null or completed_downloads < 0 then
    raise exception 'completed download count must be nonnegative'
      using errcode = '23514';
  end if;
  -- Use elapsed seconds rather than calendar intervals (DST must not change
  -- the duration of a ten-day policy window). Equality is intentionally legal.
  if extract(epoch from (evaluated_at - package_created_at)) > max_age::numeric * 86400 then
    raise exception 'package cannot be made public after the age limit'
      using errcode = 'ZD001';
  end if;
  if completed_downloads > max_downloads then
    raise exception 'package cannot be made public after the download limit'
      using errcode = 'ZD002';
  end if;
end;
$$;

create or replace function public.zed_enforce_package_policy_facts()
returns trigger
language plpgsql
set search_path = pg_catalog, public
as $$
begin
  if tg_op = 'INSERT' then
    if isfinite(new.created_at) is not true or new.created_at > clock_timestamp() then
      raise exception 'package creation time must be finite and not in the future'
        using errcode = '23514';
    end if;
  else
    if new.created_at is distinct from old.created_at then
      raise exception 'package creation time is immutable'
        using errcode = '23514';
    end if;
    if new.download_count < old.download_count then
      raise exception 'lifetime download count cannot decrease'
        using errcode = '23514';
    end if;
  end if;
  return new;
end;
$$;

drop trigger if exists zed_packages_policy_facts_guard on public.zed_packages;
create trigger zed_packages_policy_facts_guard
  before insert or update of created_at, download_count on public.zed_packages
  for each row execute function public.zed_enforce_package_policy_facts();

-- Replace the function already used by zed_packages_visibility_guard.
-- PostgreSQL owns the target row lock before this BEFORE UPDATE trigger runs.
create or replace function public.zed_enforce_package_visibility_transition()
returns trigger
language plpgsql
set search_path = pg_catalog, public
as $$
declare
  decision_at timestamptz;
begin
  if old.visibility = 'public' and new.visibility <> 'public' then
    raise exception 'public package % cannot become non-public', old.id
      using errcode = 'ZD003';
  end if;
  if old.visibility = 'public' or new.visibility <> 'public' then
    return new;
  end if;

  -- now()/CURRENT_TIMESTAMP is the start of the transaction, which may be
  -- older than a lock wait or a long-running transaction. Evaluate only after
  -- locking, using one wall-clock sample for both the check and audit field.
  decision_at := clock_timestamp();
  perform public.zed_assert_public_conversion(
    old.created_at,
    decision_at,
    greatest(old.download_count, new.download_count)
  );
  new.visibility_changed_at := decision_at;
  return new;
end;
$$;
