-- Product-owned event ledger feeding immediate package email and digests.
-- Detection is idempotent and independent from delivery so users may choose
-- immediate alerts, digests, both, or neither without losing event history.

create table if not exists zed_package_notification_events (
  id uuid primary key default gen_random_uuid(),
  package_id uuid not null,
  event_type varchar(24) not null,
  event_key varchar(255) not null,
  previous_version varchar(128),
  current_version varchar(128) not null,
  security_advisory_id uuid,
  occurred_at timestamptz not null,
  metadata jsonb default '{}'::jsonb not null,
  created_at timestamptz default now() not null,
  constraint zed_package_notification_events_package_fk
    foreign key (package_id) references zed_packages(id) on delete cascade,
  constraint zed_package_notification_events_advisory_fk
    foreign key (security_advisory_id) references zed_package_security_advisories(id) on delete cascade,
  constraint zed_package_notification_events_type_chk
    check (event_type in ('major_release', 'security_patch')),
  constraint zed_package_notification_events_key_size_chk
    check (octet_length(event_key) between 1 and 255),
  constraint zed_package_notification_events_previous_version_size_chk
    check (previous_version is null or octet_length(previous_version) between 1 and 128),
  constraint zed_package_notification_events_current_version_size_chk
    check (octet_length(current_version) between 1 and 128),
  constraint zed_package_notification_events_metadata_object_chk
    check (jsonb_typeof(metadata) = 'object'),
  constraint zed_package_notification_events_security_shape_chk
    check ((event_type = 'security_patch') = (security_advisory_id is not null))
);

create unique index if not exists zed_package_notification_events_key_uq
  on zed_package_notification_events (event_key);

create index if not exists zed_package_notification_events_package_occurred_idx
  on zed_package_notification_events (package_id, occurred_at desc);

create index if not exists zed_package_notification_events_occurred_idx
  on zed_package_notification_events (occurred_at desc);
