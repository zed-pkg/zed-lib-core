-- Add an explicit lease to the rendered-email outbox so multiple dispatcher
-- replicas can recover work after a crash without racing a legitimately active
-- sender. This is still product state: it says which rendered message is
-- claimed, not which provider will deliver it.

alter table if exists zed_email_notification_outbox
  add column if not exists lease_expires_at timestamptz;

create index if not exists zed_email_notification_outbox_stale_lease_idx
  on zed_email_notification_outbox (lease_expires_at, id)
  where state = 'publishing' and lease_expires_at is not null;

do $zed_email_outbox_lease_constraint$
begin
  if not exists (
    select 1 from pg_constraint
     where conrelid = 'zed_email_notification_outbox'::regclass
       and conname = 'zed_email_notification_outbox_lease_state_chk'
  ) then
    alter table zed_email_notification_outbox
      add constraint zed_email_notification_outbox_lease_state_chk
      check ((state = 'publishing') = (lease_expires_at is not null))
      not valid;
  end if;
end
$zed_email_outbox_lease_constraint$;
