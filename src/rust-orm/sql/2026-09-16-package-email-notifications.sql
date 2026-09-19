-- Durable package email notification state for zed-pkg.
--
-- Product semantics live here: favorites, user email preferences, security
-- advisories, and the immutable rendered-email outbox. Delivery credentials,
-- sender identity, retry transport, and provider-specific behavior do NOT live
-- in zed-pkg; generic delivery is delegated to fanwaave.

create table if not exists zed_package_favorites (
  user_id uuid not null,
  package_id uuid not null,
  created_at timestamptz default now() not null,
  primary key (user_id, package_id),
  constraint zed_package_favorites_user_fk
    foreign key (user_id) references zed_users(id) on delete cascade,
  constraint zed_package_favorites_package_fk
    foreign key (package_id) references zed_packages(id) on delete cascade
);

create index if not exists zed_package_favorites_package_idx
  on zed_package_favorites (package_id, user_id);

create table if not exists zed_notification_preferences (
  user_id uuid primary key,
  email_enabled boolean default true not null,
  favorite_package_email boolean default true not null,
  downloaded_package_email boolean default true not null,
  major_release_email boolean default true not null,
  security_email boolean default true not null,
  digest_email boolean default true not null,
  digest_frequency varchar(16) default 'weekly' not null,
  minimum_security_severity varchar(16) default 'high' not null,
  timezone text default 'UTC' not null,
  digest_hour smallint default 9 not null,
  unsubscribed_at timestamptz,
  created_at timestamptz default now() not null,
  updated_at timestamptz default now() not null,
  constraint zed_notification_preferences_user_fk
    foreign key (user_id) references zed_users(id) on delete cascade,
  constraint zed_notification_preferences_digest_frequency_chk
    check (digest_frequency in ('daily', 'weekly', 'never')),
  constraint zed_notification_preferences_security_severity_chk
    check (minimum_security_severity in ('moderate', 'high', 'critical')),
  constraint zed_notification_preferences_timezone_size_chk
    check (octet_length(timezone) between 1 and 96),
  constraint zed_notification_preferences_digest_hour_chk
    check (digest_hour between 0 and 23),
  constraint zed_notification_preferences_unsubscribe_chk
    check ((unsubscribed_at is null) or email_enabled = false)
);

drop trigger if exists zed_notification_preferences_touch on zed_notification_preferences;
create trigger zed_notification_preferences_touch
  before update on zed_notification_preferences
  for each row execute function zed_touch_updated_at();

create table if not exists zed_package_security_advisories (
  id uuid primary key default gen_random_uuid(),
  package_id uuid not null,
  advisory_id varchar(160) not null,
  advisory_url text not null,
  affected_version varchar(128) not null,
  fixed_version varchar(128) not null,
  severity varchar(16) not null,
  summary text,
  metadata jsonb default '{}'::jsonb not null,
  published_at timestamptz default now() not null,
  created_at timestamptz default now() not null,
  constraint zed_package_security_advisories_package_fk
    foreign key (package_id) references zed_packages(id) on delete cascade,
  constraint zed_package_security_advisories_id_size_chk
    check (octet_length(advisory_id) between 1 and 160),
  constraint zed_package_security_advisories_url_size_chk
    check (octet_length(advisory_url) between 1 and 2048),
  constraint zed_package_security_advisories_version_size_chk
    check (octet_length(affected_version) between 1 and 128
       and octet_length(fixed_version) between 1 and 128),
  constraint zed_package_security_advisories_severity_chk
    check (severity in ('moderate', 'high', 'critical')),
  constraint zed_package_security_advisories_summary_size_chk
    check (summary is null or octet_length(summary) <= 8192),
  constraint zed_package_security_advisories_metadata_object_chk
    check (jsonb_typeof(metadata) = 'object')
);

create unique index if not exists zed_package_security_advisories_identity_uq
  on zed_package_security_advisories (package_id, advisory_id, fixed_version);

create index if not exists zed_package_security_advisories_package_published_idx
  on zed_package_security_advisories (package_id, published_at desc);

create table if not exists zed_email_notification_outbox (
  id uuid primary key default gen_random_uuid(),
  user_id uuid not null,
  package_id uuid,
  event_type varchar(24) not null,
  idempotency_key varchar(255) not null,
  recipient_email text not null,
  recipient_name text,
  subject text not null,
  text_body text not null,
  html_body text not null,
  state varchar(16) default 'pending' not null,
  attempt_count integer default 0 not null,
  available_at timestamptz default now() not null,
  queued_at timestamptz default now() not null,
  published_at timestamptz,
  completed_at timestamptz,
  provider_message_id text,
  last_error_code varchar(128),
  metadata jsonb default '{}'::jsonb not null,
  constraint zed_email_notification_outbox_user_fk
    foreign key (user_id) references zed_users(id) on delete cascade,
  constraint zed_email_notification_outbox_package_fk
    foreign key (package_id) references zed_packages(id) on delete cascade,
  constraint zed_email_notification_outbox_event_type_chk
    check (event_type in ('major_release', 'security_patch', 'digest')),
  constraint zed_email_notification_outbox_idempotency_size_chk
    check (octet_length(idempotency_key) between 1 and 255),
  constraint zed_email_notification_outbox_email_size_chk
    check (octet_length(recipient_email) between 3 and 320),
  constraint zed_email_notification_outbox_recipient_name_size_chk
    check (recipient_name is null or octet_length(recipient_name) <= 200),
  constraint zed_email_notification_outbox_subject_size_chk
    check (octet_length(subject) between 1 and 998),
  constraint zed_email_notification_outbox_text_size_chk
    check (octet_length(text_body) between 1 and 1048576),
  constraint zed_email_notification_outbox_html_size_chk
    check (octet_length(html_body) between 1 and 1048576),
  constraint zed_email_notification_outbox_state_chk
    check (state in ('pending', 'publishing', 'published', 'delivered', 'failed', 'suppressed')),
  constraint zed_email_notification_outbox_attempt_count_chk
    check (attempt_count >= 0),
  constraint zed_email_notification_outbox_metadata_object_chk
    check (jsonb_typeof(metadata) = 'object'),
  constraint zed_email_notification_outbox_published_state_chk
    check (published_at is null or state in ('published', 'delivered', 'failed')),
  constraint zed_email_notification_outbox_completed_state_chk
    check (completed_at is null or state in ('delivered', 'failed', 'suppressed'))
);

create unique index if not exists zed_email_notification_outbox_idempotency_uq
  on zed_email_notification_outbox (idempotency_key);

create index if not exists zed_email_notification_outbox_pending_idx
  on zed_email_notification_outbox (available_at, queued_at, id)
  where state = 'pending';

create index if not exists zed_email_notification_outbox_user_idx
  on zed_email_notification_outbox (user_id, queued_at desc);

create index if not exists zed_email_notification_outbox_package_idx
  on zed_email_notification_outbox (package_id, queued_at desc)
  where package_id is not null;
