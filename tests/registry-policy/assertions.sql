\set ON_ERROR_STOP on
begin;
-- Pure comparison checks avoid pretending SQL statements execute at the same
-- microsecond. These are the exact inclusive age and count boundaries.
select public.zed_assert_public_conversion('2026-01-01Z', '2026-01-11Z', 50);
select public.zed_assert_public_conversion('2026-01-01Z', '2026-01-01Z', 0);
do $$
begin
  begin
    perform public.zed_assert_public_conversion('2026-01-01Z', '2026-01-11 00:00:00.000001Z', 0);
    raise exception 'age boundary was accepted';
  exception when sqlstate 'ZD001' then null;
  end;
  begin
    perform public.zed_assert_public_conversion('2026-01-01Z', '2026-01-01Z', 51);
    raise exception 'download boundary was accepted';
  exception when sqlstate 'ZD002' then null;
  end;
  begin
    perform public.zed_assert_public_conversion('infinity', '2026-01-01Z', 0);
    raise exception 'nonfinite timestamp was accepted';
  exception when check_violation then null;
  end;
  begin
    perform public.zed_assert_public_conversion('2026-01-02Z', '2026-01-01Z', 0);
    raise exception 'future timestamp was accepted';
  exception when check_violation then null;
  end;
  begin
    perform public.zed_assert_public_conversion('2026-01-01Z', '2026-01-01Z', -1);
    raise exception 'negative count was accepted';
  exception when check_violation then null;
  end;
end;
$$;

insert into public.zed_orgs (id, slug, name)
values ('e1111111-1111-4111-8111-111111111111', 'policy-facts-test', 'Policy facts test');
insert into public.zed_packages (id, org_id, name, visibility, download_count)
values ('e2222222-2222-4222-8222-222222222222',
        'e1111111-1111-4111-8111-111111111111', 'facts-test', 'private', 50);
do $$
begin
  begin
    update public.zed_packages set created_at = created_at + interval '1 microsecond'
    where id = 'e2222222-2222-4222-8222-222222222222';
    raise exception 'creation-time rewrite was accepted';
  exception when check_violation then null;
  end;
  begin
    update public.zed_packages set download_count = 49
    where id = 'e2222222-2222-4222-8222-222222222222';
    raise exception 'download-count rollback was accepted';
  exception when check_violation then null;
  end;
  begin
    update public.zed_packages set visibility = 'public', download_count = 51
    where id = 'e2222222-2222-4222-8222-222222222222';
    raise exception 'same-statement counter bypass was accepted';
  exception when sqlstate 'ZD002' then null;
  end;
end;
$$;
update public.zed_packages set visibility = 'internal', download_count = 51
where id = 'e2222222-2222-4222-8222-222222222222';
do $$
begin
  begin
    update public.zed_packages set visibility = 'public'
    where id = 'e2222222-2222-4222-8222-222222222222';
    raise exception 'internal-visibility bypass was accepted';
  exception when sqlstate 'ZD002' then null;
  end;
end;
$$;
insert into public.zed_packages (id, org_id, name, download_count)
values ('e3333333-3333-4333-8333-333333333333',
        'e1111111-1111-4111-8111-111111111111', 'public-test', 50);
update public.zed_packages set visibility = 'public'
where id = 'e3333333-3333-4333-8333-333333333333';
do $$
begin
  begin
    update public.zed_packages set visibility = 'private'
    where id = 'e3333333-3333-4333-8333-333333333333';
    raise exception 'public visibility was reversed';
  exception when sqlstate 'ZD003' then null;
  end;
  if (select download_count from public.zed_packages
      where id = 'e3333333-3333-4333-8333-333333333333') <> 50 then
    raise exception 'policy check mutated download accounting';
  end if;
end;
$$;
rollback;
