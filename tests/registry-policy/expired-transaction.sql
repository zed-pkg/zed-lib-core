\set ON_ERROR_STOP on
begin;
insert into public.zed_orgs (id, slug, name)
values ('e1111111-1111-4111-8111-111111111111', 'policy-clock-test', 'Policy clock test');
insert into public.zed_packages (id, org_id, name, created_at)
values ('e2222222-2222-4222-8222-222222222222',
        'e1111111-1111-4111-8111-111111111111', 'clock-test',
        now() - interval '240 hours' + interval '500 milliseconds');
select pg_sleep(0.8);
do $$
begin
  begin
    update public.zed_packages set visibility = 'public'
    where id = 'e2222222-2222-4222-8222-222222222222';
  exception when sqlstate 'ZD001' then
    return;
  end;
  raise exception 'REGRESSION: expired transaction was accepted';
end;
$$;
rollback;
