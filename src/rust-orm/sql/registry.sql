-- ════════════════════════════════════════════════════════════════════════════
-- zed_* — package registry control plane for zed-pkg, served by
-- zed-api-server.rs (read/write) and zed-web-server.rs (SELECT-only role).
-- ----------------------------------------------------------------------------
-- Every table and function carries the `zed_` prefix so the registry cannot
-- collide with the other product orgs sharing the shared-platform RDS
-- database, matching the sibling vcs.sql segment in this org. A dedicated
-- Postgres schema was the first choice and was rejected: the contract keys
-- tables by BARE name — sql-contract.mjs rejects a duplicate outright, and
-- generate.mjs derives every generated Rust struct, Zod schema, Drizzle table,
-- and TypeORM entity from it — so `zed.orgs` would collide with
-- `fiducia.orgs` in the generated adapters even though Postgres would be
-- happy. Prefixing keeps the collision from existing at all.
--
-- The sibling vcs.sql segment (vcs_* in public) is the separate dd-git-rs
-- mirror registry and is deliberately NOT folded in here.
--
-- IDENTITY. Supabase Auth is the identity provider (signup/login/OAuth/email);
-- shared-auth-server.rs verifies the Supabase JWT, owns the principal record,
-- and issues the session cookie. Sessions therefore live in the shared_auth
-- schema on the dedicated auth instances — customer principals on
-- `customer-auth`, operator/admin principals on `admin-auth` — never here.
-- zed_users.shared_auth_subject carries the shared_auth principal id, and
-- zed_users.auth_realm records which instance that principal belongs to.
-- It is a CROSS-INSTANCE reference: there is intentionally no foreign key,
-- because the auth instances are separate RDS databases. Integrity is the
-- application's responsibility (see zed-lib).
--
-- EMBEDDINGS. zed_entity_embeddings stores vectors as a jsonb array plus an
-- explicit dimension count, matching agent_context_embeddings. The `vector`
-- extension and hnsw indexes are intentionally absent from this canonical
-- contract; an ANN index, if it is ever needed, is created at runtime by the
-- owning adapter rather than being pinned here.
--
-- VISIBILITY. A private package may be promoted to public only while it is
-- young and lightly consumed: not older than
-- zed_public_conversion_max_age_days() (10) and not past
-- zed_public_conversion_max_downloads() (50) recorded downloads. The rule is
-- enforced by a BEFORE UPDATE trigger so no writer can bypass it; both limits
-- are exposed as immutable functions so the API tier pre-checks against the
-- same numbers instead of hardcoding its own copy. Violations raise the
-- dedicated SQLSTATEs ZD001 (too old) and ZD002 (too many downloads) so
-- callers can map them to a 409 without parsing message text. Once public, a
-- package cannot become non-public; that irreversible transition raises ZD003.
-- ════════════════════════════════════════════════════════════════════════════

-- ─────────────────────────────────────────────────────────────────────────────
-- Shared helpers.
-- ─────────────────────────────────────────────────────────────────────────────

-- Single source of truth for the private→public promotion window. The API tier
-- reads these rather than embedding 10/50 in Rust, so changing the policy is a
-- one-line contract change instead of a coordinated multi-repo deploy.
create or replace function zed_public_conversion_max_age_days()
returns integer
language sql
immutable
as $$ select 10 $$;

create or replace function zed_public_conversion_max_downloads()
returns bigint
language sql
immutable
as $$ select 50::bigint $$;

create or replace function zed_touch_updated_at()
returns trigger
language plpgsql
set search_path = pg_catalog, public
as $$
begin
  new.updated_at := now();
  return new;
end;
$$;

-- ─────────────────────────────────────────────────────────────────────────────
-- Accounts: users, orgs, membership, invitations.
-- ─────────────────────────────────────────────────────────────────────────────

create table if not exists zed_users (
  id uuid primary key default gen_random_uuid(),
  -- shared_auth.principals.shared_user_id on the realm's auth instance.
  -- Cross-instance: no foreign key by design (see header).
  shared_auth_subject uuid not null,
  auth_realm varchar(16) default 'customer' not null,
  email text,
  display_name text,
  avatar_url text,
  settings jsonb default '{}'::jsonb not null,
  is_soft_deleted boolean default false not null,
  created_at timestamptz default now() not null,
  updated_at timestamptz default now() not null,
  constraint zed_users_auth_realm_chk
    check (auth_realm in ('customer', 'admin')),
  constraint zed_users_email_size_chk
    check (email is null or octet_length(email) between 3 and 320),
  constraint zed_users_display_name_size_chk
    check (display_name is null or octet_length(display_name) <= 200),
  constraint zed_users_avatar_url_size_chk
    check (avatar_url is null or octet_length(avatar_url) <= 2048),
  constraint zed_users_settings_object_chk
    check (jsonb_typeof(settings) = 'object')
);

-- A principal id is only unique within its own auth instance, so the realm is
-- part of the key.
create unique index if not exists zed_users_subject_realm_uq
  on zed_users (auth_realm, shared_auth_subject);

create unique index if not exists zed_users_email_active_uq
  on zed_users (lower(email))
  where email is not null and is_soft_deleted = false;

drop trigger if exists zed_users_touch on zed_users;
create trigger zed_users_touch
  before update on zed_users
  for each row execute function zed_touch_updated_at();

create table if not exists zed_orgs (
  id uuid primary key default gen_random_uuid(),
  slug varchar(64) not null,
  name text not null,
  description text,
  settings jsonb default '{}'::jsonb not null,
  created_by_user_id uuid,
  is_soft_deleted boolean default false not null,
  created_at timestamptz default now() not null,
  updated_at timestamptz default now() not null,
  constraint zed_orgs_slug_format_chk
    check (slug ~ '^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$'),
  constraint zed_orgs_name_size_chk
    check (octet_length(name) between 1 and 200),
  constraint zed_orgs_description_size_chk
    check (description is null or octet_length(description) <= 4096),
  constraint zed_orgs_settings_object_chk
    check (jsonb_typeof(settings) = 'object')
);

create unique index if not exists zed_orgs_slug_active_uq
  on zed_orgs (slug)
  where is_soft_deleted = false;

create index if not exists zed_orgs_created_by_idx
  on zed_orgs (created_by_user_id)
  where created_by_user_id is not null;

drop trigger if exists zed_orgs_touch on zed_orgs;
create trigger zed_orgs_touch
  before update on zed_orgs
  for each row execute function zed_touch_updated_at();

create table if not exists zed_org_members (
  org_id uuid not null,
  user_id uuid not null,
  role varchar(16) not null,
  created_at timestamptz default now() not null,
  updated_at timestamptz default now() not null,
  primary key (org_id, user_id),
  constraint zed_org_members_role_chk
    check (role in ('owner', 'admin', 'member', 'reader'))
);

-- Drives the "all orgs for the logged-in user" home page query.
create index if not exists zed_org_members_user_idx
  on zed_org_members (user_id, org_id);

drop trigger if exists zed_org_members_touch on zed_org_members;
create trigger zed_org_members_touch
  before update on zed_org_members
  for each row execute function zed_touch_updated_at();

create table if not exists zed_org_invitations (
  id uuid primary key default gen_random_uuid(),
  org_id uuid not null,
  invited_by_user_id uuid not null,
  email text not null,
  role varchar(16) not null,
  token_hash varchar(64) not null,
  expires_at timestamptz not null,
  accepted_at timestamptz,
  accepted_by_user_id uuid,
  revoked_at timestamptz,
  created_at timestamptz default now() not null,
  constraint zed_org_invitations_role_chk
    check (role in ('admin', 'member', 'reader')),
  constraint zed_org_invitations_email_size_chk
    check (octet_length(email) between 3 and 320),
  -- Only the SHA-256 of the invite token is durable; the token itself is shown
  -- to the inviter once and never stored.
  constraint zed_org_invitations_token_hash_chk
    check (token_hash ~ '^[a-f0-9]{64}$'),
  constraint zed_org_invitations_accepted_chk
    check ((accepted_at is null) = (accepted_by_user_id is null))
);

create unique index if not exists zed_org_invitations_token_hash_uq
  on zed_org_invitations (token_hash);

create unique index if not exists zed_org_invitations_pending_uq
  on zed_org_invitations (org_id, lower(email))
  where accepted_at is null and revoked_at is null;

create index if not exists zed_org_invitations_org_idx
  on zed_org_invitations (org_id, created_at desc);

-- ─────────────────────────────────────────────────────────────────────────────
-- Projects: an org-scoped grouping that owns packages.
-- ─────────────────────────────────────────────────────────────────────────────

create table if not exists zed_projects (
  id uuid primary key default gen_random_uuid(),
  org_id uuid not null,
  slug varchar(64) not null,
  name text not null,
  description text,
  visibility varchar(16) default 'private' not null,
  settings jsonb default '{}'::jsonb not null,
  created_by_user_id uuid,
  is_soft_deleted boolean default false not null,
  created_at timestamptz default now() not null,
  updated_at timestamptz default now() not null,
  constraint zed_projects_slug_format_chk
    check (slug ~ '^[a-z0-9][a-z0-9._-]{0,62}[a-z0-9]$'),
  constraint zed_projects_name_size_chk
    check (octet_length(name) between 1 and 200),
  constraint zed_projects_description_size_chk
    check (description is null or octet_length(description) <= 4096),
  constraint zed_projects_visibility_chk
    check (visibility in ('private', 'internal', 'public')),
  constraint zed_projects_settings_object_chk
    check (jsonb_typeof(settings) = 'object')
);

create unique index if not exists zed_projects_org_slug_active_uq
  on zed_projects (org_id, slug)
  where is_soft_deleted = false;

create index if not exists zed_projects_org_idx
  on zed_projects (org_id, updated_at desc);

drop trigger if exists zed_projects_touch on zed_projects;
create trigger zed_projects_touch
  before update on zed_projects
  for each row execute function zed_touch_updated_at();

create table if not exists zed_project_members (
  project_id uuid not null,
  user_id uuid not null,
  role varchar(16) not null,
  created_at timestamptz default now() not null,
  updated_at timestamptz default now() not null,
  primary key (project_id, user_id),
  constraint zed_project_members_role_chk
    check (role in ('owner', 'admin', 'member', 'reader'))
);

create index if not exists zed_project_members_user_idx
  on zed_project_members (user_id, project_id);

drop trigger if exists zed_project_members_touch on zed_project_members;
create trigger zed_project_members_touch
  before update on zed_project_members
  for each row execute function zed_touch_updated_at();

create table if not exists zed_project_invitations (
  id uuid primary key default gen_random_uuid(),
  project_id uuid not null,
  invited_by_user_id uuid not null,
  email text not null,
  role varchar(16) not null,
  token_hash varchar(64) not null,
  expires_at timestamptz not null,
  accepted_at timestamptz,
  accepted_by_user_id uuid,
  revoked_at timestamptz,
  created_at timestamptz default now() not null,
  constraint zed_project_invitations_role_chk
    check (role in ('admin', 'member', 'reader')),
  constraint zed_project_invitations_email_size_chk
    check (octet_length(email) between 3 and 320),
  constraint zed_project_invitations_token_hash_chk
    check (token_hash ~ '^[a-f0-9]{64}$'),
  constraint zed_project_invitations_accepted_chk
    check ((accepted_at is null) = (accepted_by_user_id is null))
);

create unique index if not exists zed_project_invitations_token_hash_uq
  on zed_project_invitations (token_hash);

create unique index if not exists zed_project_invitations_pending_uq
  on zed_project_invitations (project_id, lower(email))
  where accepted_at is null and revoked_at is null;

create index if not exists zed_project_invitations_project_idx
  on zed_project_invitations (project_id, created_at desc);

-- ─────────────────────────────────────────────────────────────────────────────
-- Packages and versions.
--
-- download_count is a denormalized counter maintained by a trigger on
-- zed_package_downloads. It exists so the visibility trigger can evaluate the
-- 50-download limit with a single row read instead of an aggregate over the
-- event table, and it is the authoritative number for that rule.
-- ─────────────────────────────────────────────────────────────────────────────

create table if not exists zed_packages (
  id uuid primary key default gen_random_uuid(),
  org_id uuid not null,
  project_id uuid,
  name varchar(128) not null,
  description text,
  visibility varchar(16) default 'private' not null,
  vcs varchar(16) default 'git' not null,
  repo_url text default '' not null,
  homepage_url text,
  keywords jsonb default '[]'::jsonb not null,
  config jsonb default '{}'::jsonb not null,
  download_count bigint default 0 not null,
  version_count integer default 0 not null,
  latest_version varchar(128),
  first_published_at timestamptz,
  visibility_changed_at timestamptz,
  created_by_user_id uuid,
  is_soft_deleted boolean default false not null,
  created_at timestamptz default now() not null,
  updated_at timestamptz default now() not null,
  constraint zed_packages_name_format_chk
    check (name ~ '^[a-z0-9][a-z0-9._-]{0,126}[a-z0-9]$'),
  constraint zed_packages_description_size_chk
    check (description is null or octet_length(description) <= 4096),
  constraint zed_packages_visibility_chk
    check (visibility in ('private', 'internal', 'public')),
  constraint zed_packages_vcs_chk
    check (vcs in ('git', 'hg', 'svn', 'fossil')),
  constraint zed_packages_repo_url_size_chk
    check (octet_length(repo_url) <= 2048),
  constraint zed_packages_homepage_url_size_chk
    check (homepage_url is null or octet_length(homepage_url) <= 2048),
  constraint zed_packages_keywords_array_chk
    check (jsonb_typeof(keywords) = 'array'),
  constraint zed_packages_config_object_chk
    check (jsonb_typeof(config) = 'object'),
  constraint zed_packages_download_count_chk
    check (download_count >= 0),
  constraint zed_packages_version_count_chk
    check (version_count >= 0)
);

-- Package URLs are org-scoped (/p/{org}/{name}); a project is an optional
-- grouping inside the org and deliberately does NOT widen the namespace.
create unique index if not exists zed_packages_org_name_active_uq
  on zed_packages (org_id, name)
  where is_soft_deleted = false;

create index if not exists zed_packages_project_idx
  on zed_packages (project_id)
  where project_id is not null;

create index if not exists zed_packages_public_recent_idx
  on zed_packages (created_at desc)
  where visibility = 'public' and is_soft_deleted = false;

create index if not exists zed_packages_download_count_idx
  on zed_packages (download_count desc)
  where is_soft_deleted = false;

drop trigger if exists zed_packages_touch on zed_packages;
create trigger zed_packages_touch
  before update on zed_packages
  for each row execute function zed_touch_updated_at();

create table if not exists zed_package_versions (
  id uuid primary key default gen_random_uuid(),
  package_id uuid not null,
  version varchar(128) not null,
  version_scheme varchar(16) default 'semver' not null,
  sha256 varchar(64) not null,
  size_bytes bigint not null,
  format varchar(16) not null,
  vcs_tag varchar(160),
  vcs_commit varchar(120),
  artifact_key text not null,
  manifest jsonb default '{}'::jsonb not null,
  download_count bigint default 0 not null,
  yanked boolean default false not null,
  yanked_at timestamptz,
  yanked_reason text,
  published_by_user_id uuid,
  published_at timestamptz default now() not null,
  constraint zed_package_versions_version_size_chk
    check (octet_length(version) between 1 and 128),
  constraint zed_package_versions_scheme_chk
    check (version_scheme in ('semver', 'calver', 'opaque')),
  constraint zed_package_versions_sha256_chk
    check (sha256 ~ '^[a-f0-9]{64}$'),
  constraint zed_package_versions_size_chk
    check (size_bytes >= 0),
  constraint zed_package_versions_format_chk
    check (format in ('tar.gz', 'tar.zst', 'zip')),
  constraint zed_package_versions_artifact_key_size_chk
    check (octet_length(artifact_key) between 1 and 1024),
  constraint zed_package_versions_manifest_object_chk
    check (jsonb_typeof(manifest) = 'object'),
  constraint zed_package_versions_download_count_chk
    check (download_count >= 0),
  constraint zed_package_versions_yanked_chk
    check (yanked = (yanked_at is not null))
);

create unique index if not exists zed_package_versions_package_version_uq
  on zed_package_versions (package_id, version);

create index if not exists zed_package_versions_package_published_idx
  on zed_package_versions (package_id, published_at desc);

create index if not exists zed_package_versions_sha256_idx
  on zed_package_versions (sha256);

-- ─────────────────────────────────────────────────────────────────────────────
-- Immutable dependency-graph artifacts and normalized edges.
--
-- The JSON document is the lossless authority used for downloads. Edges are a
-- derived relational index committed in the same transaction so reverse
-- impact, neighborhood, path, and organization/project aggregate queries do
-- not repeatedly scan package manifests or decode graph JSON. Declared graphs
-- have exactly one immutable artifact per package version. Resolved graphs may
-- have many target/feature/checkpoint-specific artifacts, each addressed by
-- its semantic digest.
-- ─────────────────────────────────────────────────────────────────────────────

create table if not exists zed_dependency_graph_artifacts (
  id uuid primary key default gen_random_uuid(),
  root_package_version_id uuid not null,
  graph_kind varchar(16) not null,
  schema_version varchar(96) not null,
  graph_digest varchar(71) not null,
  resolver_name varchar(120),
  resolver_version varchar(120),
  resolution_input_digest varchar(71),
  registry_checkpoint text,
  target jsonb default '{}'::jsonb not null,
  enabled_features jsonb default '[]'::jsonb not null,
  document jsonb not null,
  node_count integer not null,
  edge_count integer not null,
  max_depth integer not null,
  cycle_count integer default 0 not null,
  created_at timestamptz default now() not null,
  sealed_at timestamptz,
  constraint zed_dependency_graph_artifacts_kind_chk
    check (graph_kind in ('declared', 'resolved')),
  constraint zed_dependency_graph_artifacts_schema_size_chk
    check (octet_length(schema_version) between 1 and 96),
  constraint zed_dependency_graph_artifacts_digest_chk
    check (graph_digest ~ '^sha256:[a-f0-9]{64}$'),
  constraint zed_dependency_graph_artifacts_input_digest_chk
    check (resolution_input_digest is null or resolution_input_digest ~ '^sha256:[a-f0-9]{64}$'),
  constraint zed_dependency_graph_artifacts_resolver_name_size_chk
    check (resolver_name is null or octet_length(resolver_name) between 1 and 120),
  constraint zed_dependency_graph_artifacts_resolver_version_size_chk
    check (resolver_version is null or octet_length(resolver_version) between 1 and 120),
  constraint zed_dependency_graph_artifacts_checkpoint_size_chk
    check (registry_checkpoint is null or octet_length(registry_checkpoint) <= 1024),
  constraint zed_dependency_graph_artifacts_target_object_chk
    check (jsonb_typeof(target) = 'object'),
  constraint zed_dependency_graph_artifacts_features_array_chk
    check (jsonb_typeof(enabled_features) = 'array'),
  constraint zed_dependency_graph_artifacts_document_object_chk
    check (jsonb_typeof(document) = 'object'),
  constraint zed_dependency_graph_artifacts_document_binding_chk
    check (document ->> 'schema' is not distinct from schema_version
       and document ->> 'graph_digest' is not distinct from graph_digest
       and document ->> 'view' is not distinct from graph_kind
       and (graph_kind <> 'resolved'
         or document #>> '{provenance,resolver_version}' is not distinct from resolver_version)),
  constraint zed_dependency_graph_artifacts_counts_chk
    check (node_count >= 1 and edge_count >= 0 and max_depth >= 0 and cycle_count >= 0),
  constraint zed_dependency_graph_artifacts_default_limits_chk
    check (node_count <= 50000 and edge_count <= 500000 and max_depth <= node_count),
  constraint zed_dependency_graph_artifacts_declared_metadata_chk
    check (graph_kind <> 'declared'
      or (registry_checkpoint is null and target = '{}'::jsonb and enabled_features = '[]'::jsonb)),
  constraint zed_dependency_graph_artifacts_resolved_features_chk
    check (graph_kind <> 'resolved'
      or enabled_features = coalesce(document #> '{provenance,enabled_features}', '[]'::jsonb)),
  constraint zed_dependency_graph_artifacts_sealed_at_chk
    check (sealed_at is null or sealed_at >= created_at),
  constraint zed_dependency_graph_artifacts_resolution_shape_chk
    check (
      (graph_kind = 'declared'
        and resolver_name is null
        and resolver_version is null
        and resolution_input_digest is null)
      or
      (graph_kind = 'resolved'
        and resolver_name is not null
        and resolver_version is not null
        and resolution_input_digest is not null)
  )
);

-- `create table if not exists` does not evolve a branch-preview database that
-- created the graph table before sealing was introduced.
alter table if exists zed_dependency_graph_artifacts
  add column if not exists sealed_at timestamptz;

do $zed_graph_sealed_at_constraint$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_dependency_graph_artifacts'::regclass
      and conname = 'zed_dependency_graph_artifacts_sealed_at_chk'
  ) then
    alter table zed_dependency_graph_artifacts
      add constraint zed_dependency_graph_artifacts_sealed_at_chk
      check (sealed_at is null or sealed_at >= created_at);
  end if;
end
$zed_graph_sealed_at_constraint$;

-- These bindings were added after the first graph-table preview. `not valid`
-- keeps a snapshot reapply operational if preview data exists while enforcing
-- the authority contract for every new or changed row. A fresh database gets
-- the equivalent validated constraints from `create table` above.
do $zed_graph_authority_constraints$
begin
  if not exists (
    select 1 from pg_constraint
    where conrelid = 'zed_dependency_graph_artifacts'::regclass
      and conname = 'zed_dependency_graph_artifacts_document_binding_chk'
  ) then
    alter table zed_dependency_graph_artifacts
      add constraint zed_dependency_graph_artifacts_document_binding_chk
      check (document ->> 'schema' is not distinct from schema_version
         and document ->> 'graph_digest' is not distinct from graph_digest
         and document ->> 'view' is not distinct from graph_kind
         and (graph_kind <> 'resolved'
           or document #>> '{provenance,resolver_version}' is not distinct from resolver_version))
      not valid;
  end if;

  if not exists (
    select 1 from pg_constraint
    where conrelid = 'zed_dependency_graph_artifacts'::regclass
      and conname = 'zed_dependency_graph_artifacts_default_limits_chk'
  ) then
    alter table zed_dependency_graph_artifacts
      add constraint zed_dependency_graph_artifacts_default_limits_chk
      check (node_count <= 50000 and edge_count <= 500000 and max_depth <= node_count)
      not valid;
  end if;

  if not exists (
    select 1 from pg_constraint
    where conrelid = 'zed_dependency_graph_artifacts'::regclass
      and conname = 'zed_dependency_graph_artifacts_declared_metadata_chk'
  ) then
    alter table zed_dependency_graph_artifacts
      add constraint zed_dependency_graph_artifacts_declared_metadata_chk
      check (graph_kind <> 'declared'
        or (registry_checkpoint is null and target = '{}'::jsonb and enabled_features = '[]'::jsonb))
      not valid;
  end if;

  if not exists (
    select 1 from pg_constraint
    where conrelid = 'zed_dependency_graph_artifacts'::regclass
      and conname = 'zed_dependency_graph_artifacts_resolved_features_chk'
  ) then
    alter table zed_dependency_graph_artifacts
      add constraint zed_dependency_graph_artifacts_resolved_features_chk
      check (graph_kind <> 'resolved'
        or enabled_features = coalesce(document #> '{provenance,enabled_features}', '[]'::jsonb))
      not valid;
  end if;
end
$zed_graph_authority_constraints$;

create unique index if not exists zed_dependency_graph_artifacts_digest_uq
  on zed_dependency_graph_artifacts (graph_digest);

create unique index if not exists zed_dependency_graph_artifacts_declared_root_uq
  on zed_dependency_graph_artifacts (root_package_version_id)
  where graph_kind = 'declared';

create index if not exists zed_dependency_graph_artifacts_root_created_idx
  on zed_dependency_graph_artifacts (root_package_version_id, created_at desc);

create index if not exists zed_dependency_graph_artifacts_resolved_input_idx
  on zed_dependency_graph_artifacts (resolution_input_digest)
  where graph_kind = 'resolved';

create index if not exists zed_dependency_graph_artifacts_unsealed_idx
  on zed_dependency_graph_artifacts (created_at)
  where sealed_at is null;

create table if not exists zed_dependency_graph_edges (
  id uuid primary key default gen_random_uuid(),
  graph_artifact_id uuid not null,
  ordinal integer not null,
  from_registry_id text not null,
  from_org_slug varchar(64) not null,
  from_package_name varchar(128) not null,
  from_version varchar(128),
  from_package_id uuid,
  from_package_version_id uuid,
  to_registry_id text not null,
  to_org_slug varchar(64) not null,
  to_package_name varchar(128) not null,
  to_version varchar(128),
  to_package_id uuid,
  to_package_version_id uuid,
  requirement text,
  dependency_kind varchar(16) not null,
  optional boolean default false not null,
  default_features boolean default true not null,
  features jsonb default '[]'::jsonb not null,
  target text,
  minimum_depth integer not null,
  created_at timestamptz default now() not null,
  constraint zed_dependency_graph_edges_ordinal_chk
    check (ordinal >= 0),
  constraint zed_dependency_graph_edges_registry_size_chk
    check (octet_length(from_registry_id) between 1 and 512
       and octet_length(to_registry_id) between 1 and 512),
  constraint zed_dependency_graph_edges_org_format_chk
    check (from_org_slug ~ '^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$'
       and to_org_slug ~ '^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$'),
  constraint zed_dependency_graph_edges_package_format_chk
    check (from_package_name ~ '^[a-z0-9][a-z0-9._-]{0,126}[a-z0-9]$'
       and to_package_name ~ '^[a-z0-9][a-z0-9._-]{0,126}[a-z0-9]$'),
  constraint zed_dependency_graph_edges_version_size_chk
    check ((from_version is null or octet_length(from_version) between 1 and 128)
       and (to_version is null or octet_length(to_version) between 1 and 128)),
  constraint zed_dependency_graph_edges_requirement_size_chk
    check (requirement is null or octet_length(requirement) between 1 and 1024),
  constraint zed_dependency_graph_edges_kind_chk
    check (dependency_kind in ('runtime', 'build', 'development', 'peer', 'tooling')),
  constraint zed_dependency_graph_edges_features_array_chk
    check (jsonb_typeof(features) = 'array'),
  constraint zed_dependency_graph_edges_target_size_chk
    check (target is null or octet_length(target) <= 512),
  constraint zed_dependency_graph_edges_depth_chk
    check (minimum_depth >= 1),
  constraint zed_dependency_graph_edges_source_version_chk
    check ((from_package_version_id is null) or (from_package_id is not null and from_version is not null)),
  constraint zed_dependency_graph_edges_target_version_chk
    check ((to_package_version_id is null) or (to_package_id is not null and to_version is not null))
);

create unique index if not exists zed_dependency_graph_edges_artifact_ordinal_uq
  on zed_dependency_graph_edges (graph_artifact_id, ordinal);

create index if not exists zed_dependency_graph_edges_outgoing_idx
  on zed_dependency_graph_edges (from_registry_id, from_org_slug, from_package_name, minimum_depth);

create index if not exists zed_dependency_graph_edges_incoming_idx
  on zed_dependency_graph_edges (to_registry_id, to_org_slug, to_package_name, minimum_depth);

create index if not exists zed_dependency_graph_edges_outgoing_version_idx
  on zed_dependency_graph_edges
    (from_registry_id, from_org_slug, from_package_name, from_version, minimum_depth, graph_artifact_id, ordinal)
  where from_version is not null;

create index if not exists zed_dependency_graph_edges_incoming_version_idx
  on zed_dependency_graph_edges
    (to_registry_id, to_org_slug, to_package_name, to_version, minimum_depth, graph_artifact_id, ordinal)
  where to_version is not null;

create index if not exists zed_dependency_graph_edges_from_package_idx
  on zed_dependency_graph_edges (from_package_id, graph_artifact_id)
  where from_package_id is not null;

create index if not exists zed_dependency_graph_edges_to_package_idx
  on zed_dependency_graph_edges (to_package_id, graph_artifact_id)
  where to_package_id is not null;

create index if not exists zed_dependency_graph_edges_to_version_idx
  on zed_dependency_graph_edges (to_package_version_id, graph_artifact_id)
  where to_package_version_id is not null;

create index if not exists zed_dependency_graph_edges_from_version_idx
  on zed_dependency_graph_edges (from_package_version_id, graph_artifact_id)
  where from_package_version_id is not null;

create index if not exists zed_dependency_graph_edges_unresolved_target_idx
  on zed_dependency_graph_edges (to_registry_id, to_org_slug, to_package_name)
  where to_package_version_id is null;

-- An artifact is inserted unsealed, receives its complete edge projection in
-- the same transaction, and is then sealed. Readers expose only sealed rows.
-- Once sealed, neither its document nor its derived index can be edited. A
-- whole artifact may still be deleted (including by the root-version FK); the
-- edge guard permits that cascade only after the parent row is no longer
-- visible to the trigger statement.
create or replace function zed_guard_dependency_graph_artifact_mutation()
returns trigger
language plpgsql
set search_path = pg_catalog, public
as $$
declare
  stored_edge_count bigint;
  first_ordinal integer;
  last_ordinal integer;
begin
  if tg_op = 'INSERT' then
    if new.sealed_at is not null then
      raise exception 'dependency graph artifact % must be inserted unsealed', new.id
        using errcode = 'ZD004';
    end if;
    return new;
  end if;

  if old.sealed_at is not null then
    raise exception 'sealed dependency graph artifact % is immutable', old.id
      using errcode = 'ZD004';
  end if;

  if new.sealed_at is null then
    raise exception 'dependency graph artifact % may only be updated to seal it', old.id
      using errcode = 'ZD004';
  end if;

  if (new.id, new.root_package_version_id, new.graph_kind, new.schema_version,
      new.graph_digest, new.resolver_name, new.resolver_version,
      new.resolution_input_digest, new.registry_checkpoint, new.target,
      new.enabled_features, new.document, new.node_count, new.edge_count,
      new.max_depth, new.cycle_count, new.created_at)
     is distinct from
     (old.id, old.root_package_version_id, old.graph_kind, old.schema_version,
      old.graph_digest, old.resolver_name, old.resolver_version,
      old.resolution_input_digest, old.registry_checkpoint, old.target,
      old.enabled_features, old.document, old.node_count, old.edge_count,
      old.max_depth, old.cycle_count, old.created_at) then
    raise exception 'dependency graph artifact % immutable facts changed while sealing', old.id
      using errcode = 'ZD004';
  end if;

  select count(*), min(ordinal), max(ordinal)
    into stored_edge_count, first_ordinal, last_ordinal
    from zed_dependency_graph_edges
   where graph_artifact_id = old.id;

  if stored_edge_count <> old.edge_count
     or (old.edge_count > 0 and (first_ordinal <> 0 or last_ordinal <> old.edge_count - 1)) then
    raise exception 'dependency graph artifact % cannot seal with a divergent edge index', old.id
      using errcode = 'ZD005';
  end if;
  return new;
end;
$$;

drop trigger if exists zed_dependency_graph_artifacts_immutable on zed_dependency_graph_artifacts;
create trigger zed_dependency_graph_artifacts_immutable
  before insert or update on zed_dependency_graph_artifacts
  for each row execute function zed_guard_dependency_graph_artifact_mutation();

create or replace function zed_guard_dependency_graph_edge_mutation()
returns trigger
language plpgsql
set search_path = pg_catalog, public
as $$
declare
  graph_id uuid;
  graph_is_sealed boolean;
begin
  graph_id := case when tg_op = 'DELETE' then old.graph_artifact_id else new.graph_artifact_id end;
  select sealed_at is not null
    into graph_is_sealed
    from zed_dependency_graph_artifacts
   where id = graph_id;

  if coalesce(graph_is_sealed, false) then
    raise exception 'sealed dependency graph edge index for artifact % is immutable', graph_id
      using errcode = 'ZD004';
  end if;

  if tg_op = 'UPDATE' and old.graph_artifact_id <> new.graph_artifact_id then
    select sealed_at is not null
      into graph_is_sealed
      from zed_dependency_graph_artifacts
     where id = old.graph_artifact_id;
    if coalesce(graph_is_sealed, false) then
      raise exception 'sealed dependency graph edge index for artifact % is immutable', old.graph_artifact_id
        using errcode = 'ZD004';
    end if;
  end if;

  if tg_op = 'DELETE' then
    return old;
  end if;
  return new;
end;
$$;

drop trigger if exists zed_dependency_graph_edges_immutable on zed_dependency_graph_edges;
create trigger zed_dependency_graph_edges_immutable
  before insert or update or delete on zed_dependency_graph_edges
  for each row execute function zed_guard_dependency_graph_edge_mutation();

-- ─────────────────────────────────────────────────────────────────────────────
-- Licenses.
--
-- A row with package_version_id IS NULL is the package-level default that
-- applies to versions which do not declare their own. `kind` separates a
-- recognized SPDX identifier from custom text and from all-rights-reserved
-- proprietary packages, so a private package is representable without
-- inventing a fake SPDX id.
-- ─────────────────────────────────────────────────────────────────────────────

create table if not exists zed_package_licenses (
  id uuid primary key default gen_random_uuid(),
  package_id uuid not null,
  package_version_id uuid,
  kind varchar(16) default 'spdx' not null,
  spdx_id varchar(120),
  name text,
  url text,
  text_body text,
  is_primary boolean default true not null,
  created_at timestamptz default now() not null,
  updated_at timestamptz default now() not null,
  constraint zed_package_licenses_kind_chk
    check (kind in ('spdx', 'custom', 'proprietary')),
  -- An SPDX row must carry an identifier; a custom row must carry the text or a
  -- URL pointing at it; proprietary needs neither.
  constraint zed_package_licenses_spdx_id_chk
    check (
      (kind = 'spdx' and spdx_id ~ '^[A-Za-z0-9.+()-]{1,120}$')
      or (kind <> 'spdx' and spdx_id is null)
    ),
  constraint zed_package_licenses_custom_body_chk
    check (kind <> 'custom' or text_body is not null or url is not null),
  constraint zed_package_licenses_name_size_chk
    check (name is null or octet_length(name) <= 200),
  constraint zed_package_licenses_url_size_chk
    check (url is null or octet_length(url) <= 2048),
  constraint zed_package_licenses_text_size_chk
    check (text_body is null or octet_length(text_body) <= 262144)
);

create index if not exists zed_package_licenses_package_idx
  on zed_package_licenses (package_id);

create index if not exists zed_package_licenses_version_idx
  on zed_package_licenses (package_version_id)
  where package_version_id is not null;

create index if not exists zed_package_licenses_spdx_idx
  on zed_package_licenses (spdx_id)
  where spdx_id is not null;

-- At most one primary license for the package default, and one per version.
create unique index if not exists zed_package_licenses_package_primary_uq
  on zed_package_licenses (package_id)
  where package_version_id is null and is_primary;

create unique index if not exists zed_package_licenses_version_primary_uq
  on zed_package_licenses (package_version_id)
  where package_version_id is not null and is_primary;

drop trigger if exists zed_package_licenses_touch on zed_package_licenses;
create trigger zed_package_licenses_touch
  before update on zed_package_licenses
  for each row execute function zed_touch_updated_at();

-- ─────────────────────────────────────────────────────────────────────────────
-- Search vectors.
--
-- Polymorphic over the searchable entities. entity_id is not foreign-keyed
-- because it addresses four different tables; the owning writer deletes the
-- embedding rows alongside the entity. Stored as a jsonb array + explicit
-- dimension count, per the contract's no-pgvector policy (see header).
-- ─────────────────────────────────────────────────────────────────────────────

create table if not exists zed_entity_embeddings (
  id uuid primary key default gen_random_uuid(),
  entity_type varchar(24) not null,
  entity_id uuid not null,
  org_id uuid,
  embedding_model varchar(120) not null,
  embedding jsonb not null,
  embedding_dimensions integer not null,
  -- SHA-256 of the exact text that was embedded, so a re-index is a no-op when
  -- the source text has not changed.
  content_sha256 varchar(64) not null,
  content_preview text,
  created_at timestamptz default now() not null,
  updated_at timestamptz default now() not null,
  constraint zed_entity_embeddings_entity_type_chk
    check (entity_type in ('org', 'project', 'package', 'package_version')),
  constraint zed_entity_embeddings_model_format_chk
    check (embedding_model ~ '^[A-Za-z0-9._:/-]{1,120}$'),
  constraint zed_entity_embeddings_array_chk
    check (jsonb_typeof(embedding) = 'array'),
  constraint zed_entity_embeddings_dimensions_chk
    check (embedding_dimensions > 0),
  constraint zed_entity_embeddings_dimensions_match_chk
    check (jsonb_array_length(embedding) = embedding_dimensions),
  constraint zed_entity_embeddings_sha256_chk
    check (content_sha256 ~ '^[a-f0-9]{64}$'),
  constraint zed_entity_embeddings_preview_size_chk
    check (content_preview is null or octet_length(content_preview) <= 4096)
);

create unique index if not exists zed_entity_embeddings_entity_model_sha_uq
  on zed_entity_embeddings (entity_type, entity_id, embedding_model, content_sha256);

create index if not exists zed_entity_embeddings_entity_idx
  on zed_entity_embeddings (entity_type, entity_id);

-- Semantic search is always scoped to what the caller may read, so the org is
-- the leading filter column.
create index if not exists zed_entity_embeddings_org_model_idx
  on zed_entity_embeddings (org_id, embedding_model)
  where org_id is not null;

drop trigger if exists zed_entity_embeddings_touch on zed_entity_embeddings;
create trigger zed_entity_embeddings_touch
  before update on zed_entity_embeddings
  for each row execute function zed_touch_updated_at();

-- ─────────────────────────────────────────────────────────────────────────────
-- Upload and download ledgers.
--
-- package_uploads is the publish-attempt record: it is created before bytes
-- land in object storage and keeps failed/aborted attempts for operator
-- forensics, so it is NOT one-to-one with package_versions.
-- ─────────────────────────────────────────────────────────────────────────────

create table if not exists zed_package_uploads (
  id uuid primary key default gen_random_uuid(),
  package_id uuid not null,
  -- Null until the upload is verified and a version row is created.
  package_version_id uuid,
  requested_version varchar(128) not null,
  status varchar(16) default 'pending' not null,
  storage_backend varchar(16) default 's3' not null,
  storage_key text,
  format varchar(16),
  size_bytes bigint,
  sha256 varchar(64),
  uploaded_by_user_id uuid,
  api_token_id uuid,
  client_ip_hash varchar(64),
  user_agent text,
  error text,
  started_at timestamptz default now() not null,
  completed_at timestamptz,
  created_at timestamptz default now() not null,
  updated_at timestamptz default now() not null,
  constraint zed_package_uploads_status_chk
    check (status in ('pending', 'uploading', 'stored', 'verified', 'failed', 'aborted')),
  constraint zed_package_uploads_backend_chk
    check (storage_backend in ('s3', 'r2', 'gcs', 'fs')),
  constraint zed_package_uploads_format_chk
    check (format is null or format in ('tar.gz', 'tar.zst', 'zip')),
  constraint zed_package_uploads_size_chk
    check (size_bytes is null or size_bytes >= 0),
  constraint zed_package_uploads_sha256_chk
    check (sha256 is null or sha256 ~ '^[a-f0-9]{64}$'),
  constraint zed_package_uploads_storage_key_size_chk
    check (storage_key is null or octet_length(storage_key) between 1 and 1024),
  constraint zed_package_uploads_ip_hash_chk
    check (client_ip_hash is null or client_ip_hash ~ '^[a-f0-9]{64}$'),
  constraint zed_package_uploads_user_agent_size_chk
    check (user_agent is null or octet_length(user_agent) <= 512),
  -- A verified upload must point at the version it produced; a terminal
  -- failure must not.
  constraint zed_package_uploads_verified_chk
    check (status <> 'verified' or package_version_id is not null),
  constraint zed_package_uploads_failed_chk
    check (status not in ('failed', 'aborted') or package_version_id is null),
  constraint zed_package_uploads_completed_chk
    check (status not in ('verified', 'failed', 'aborted') or completed_at is not null)
);

create index if not exists zed_package_uploads_package_idx
  on zed_package_uploads (package_id, started_at desc);

create index if not exists zed_package_uploads_version_idx
  on zed_package_uploads (package_version_id)
  where package_version_id is not null;

create index if not exists zed_package_uploads_status_idx
  on zed_package_uploads (status, started_at desc);

create index if not exists zed_package_uploads_user_idx
  on zed_package_uploads (uploaded_by_user_id, started_at desc)
  where uploaded_by_user_id is not null;

drop trigger if exists zed_package_uploads_touch on zed_package_uploads;
create trigger zed_package_uploads_touch
  before update on zed_package_uploads
  for each row execute function zed_touch_updated_at();

-- One row per served artifact or metadata fetch. downloaded_by_user_id is null
-- for anonymous public traffic; client_ip_hash is a salted SHA-256, never a raw
-- address.
create table if not exists zed_package_downloads (
  id uuid primary key default gen_random_uuid(),
  package_id uuid not null,
  package_version_id uuid,
  downloaded_by_user_id uuid,
  api_token_id uuid,
  source varchar(16) default 'cli' not null,
  format varchar(16),
  bytes_sent bigint,
  client_ip_hash varchar(64),
  user_agent text,
  created_at timestamptz default now() not null,
  constraint zed_package_downloads_source_chk
    check (source in ('cli', 'web', 'api', 'mirror', 'ci')),
  constraint zed_package_downloads_format_chk
    check (format is null or format in ('tar.gz', 'tar.zst', 'zip')),
  constraint zed_package_downloads_bytes_chk
    check (bytes_sent is null or bytes_sent >= 0),
  constraint zed_package_downloads_ip_hash_chk
    check (client_ip_hash is null or client_ip_hash ~ '^[a-f0-9]{64}$'),
  constraint zed_package_downloads_user_agent_size_chk
    check (user_agent is null or octet_length(user_agent) <= 512)
);

create index if not exists zed_package_downloads_package_idx
  on zed_package_downloads (package_id, created_at desc);

create index if not exists zed_package_downloads_version_idx
  on zed_package_downloads (package_version_id, created_at desc)
  where package_version_id is not null;

create index if not exists zed_package_downloads_user_idx
  on zed_package_downloads (downloaded_by_user_id, created_at desc)
  where downloaded_by_user_id is not null;

create index if not exists zed_package_downloads_created_at_idx
  on zed_package_downloads (created_at desc);

-- ─────────────────────────────────────────────────────────────────────────────
-- Publish tokens and audit.
-- ─────────────────────────────────────────────────────────────────────────────

create table if not exists zed_api_tokens (
  id uuid primary key default gen_random_uuid(),
  name text not null,
  -- SHA-256 of the bearer token; the plaintext is shown once at creation.
  token_hash varchar(64) not null,
  org_id uuid,
  user_id uuid,
  role varchar(16) default 'publish' not null,
  last_used_at timestamptz,
  expires_at timestamptz,
  revoked_at timestamptz,
  created_at timestamptz default now() not null,
  updated_at timestamptz default now() not null,
  constraint zed_api_tokens_name_size_chk
    check (octet_length(name) between 1 and 200),
  constraint zed_api_tokens_token_hash_chk
    check (token_hash ~ '^[a-f0-9]{64}$'),
  constraint zed_api_tokens_role_chk
    check (role in ('read', 'publish', 'admin')),
  -- A token is scoped to an org, a user, or both; never to neither.
  constraint zed_api_tokens_scope_chk
    check (org_id is not null or user_id is not null)
);

create unique index if not exists zed_api_tokens_token_hash_uq
  on zed_api_tokens (token_hash);

create index if not exists zed_api_tokens_org_idx
  on zed_api_tokens (org_id)
  where org_id is not null;

create index if not exists zed_api_tokens_user_idx
  on zed_api_tokens (user_id)
  where user_id is not null;

drop trigger if exists zed_api_tokens_touch on zed_api_tokens;
create trigger zed_api_tokens_touch
  before update on zed_api_tokens
  for each row execute function zed_touch_updated_at();

create table if not exists zed_audit_log (
  id uuid primary key default gen_random_uuid(),
  org_id uuid,
  actor_user_id uuid,
  api_token_id uuid,
  action varchar(64) not null,
  entity_type varchar(24) not null,
  entity_id uuid,
  detail jsonb default '{}'::jsonb not null,
  client_ip_hash varchar(64),
  created_at timestamptz default now() not null,
  constraint zed_audit_log_action_format_chk
    check (action ~ '^[a-z][a-z0-9_.]{0,63}$'),
  constraint zed_audit_log_entity_type_chk
    check (entity_type in ('user', 'org', 'project', 'package', 'package_version',
                           'package_license', 'api_token', 'invitation')),
  constraint zed_audit_log_detail_object_chk
    check (jsonb_typeof(detail) = 'object'),
  constraint zed_audit_log_ip_hash_chk
    check (client_ip_hash is null or client_ip_hash ~ '^[a-f0-9]{64}$')
);

create index if not exists zed_audit_log_org_idx
  on zed_audit_log (org_id, created_at desc)
  where org_id is not null;

create index if not exists zed_audit_log_entity_idx
  on zed_audit_log (entity_type, entity_id, created_at desc);

create index if not exists zed_audit_log_actor_idx
  on zed_audit_log (actor_user_id, created_at desc)
  where actor_user_id is not null;

-- ─────────────────────────────────────────────────────────────────────────────
-- Counters and the visibility policy.
-- ─────────────────────────────────────────────────────────────────────────────

-- Keep the denormalized counters on packages/package_versions in step with the
-- download ledger. Only INSERT is handled: the ledger is append-only, and
-- pruning old events must not retroactively re-open the promotion window.
create or replace function zed_bump_download_counts()
returns trigger
language plpgsql
set search_path = pg_catalog, public
as $$
begin
  update zed_packages
     set download_count = download_count + 1
   where id = new.package_id;

  if new.package_version_id is not null then
    update zed_package_versions
       set download_count = download_count + 1
     where id = new.package_version_id;
  end if;

  return null;
end;
$$;

drop trigger if exists zed_package_downloads_bump_counts on zed_package_downloads;
create trigger zed_package_downloads_bump_counts
  after insert on zed_package_downloads
  for each row execute function zed_bump_download_counts();

-- Maintain version_count / latest_version / first_published_at so the listing
-- pages do not aggregate over package_versions on every render.
create or replace function zed_refresh_package_version_rollup()
returns trigger
language plpgsql
set search_path = pg_catalog, public
as $$
declare
  target uuid := coalesce(new.package_id, old.package_id);
begin
  update zed_packages p
     set version_count = stats.total,
         latest_version = stats.latest,
         first_published_at = stats.first_at
    from (
      select count(*) as total,
             min(published_at) as first_at,
             (select v.version
                from zed_package_versions v
               where v.package_id = target and v.yanked = false
               order by v.published_at desc
               limit 1) as latest
        from zed_package_versions
       where package_id = target
    ) as stats
   where p.id = target;

  return null;
end;
$$;

drop trigger if exists zed_package_versions_rollup on zed_package_versions;
create trigger zed_package_versions_rollup
  after insert or update or delete on zed_package_versions
  for each row execute function zed_refresh_package_version_rollup();

-- The private→public promotion window. Enforced here so no writer — API
-- server, migration job, or a human at psql — can promote a package that has
-- outgrown the window. zed-lib pre-checks the same limits to return a clean
-- 409 instead of surfacing a raw database exception.
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

drop trigger if exists zed_packages_visibility_guard on zed_packages;
create trigger zed_packages_visibility_guard
  before update of visibility on zed_packages
  for each row execute function zed_enforce_package_visibility_transition();

-- ─────────────────────────────────────────────────────────────────────────────
-- Foreign keys.
--
-- Declared after every table so the segment stays order-independent within
-- itself. shared_auth principals are deliberately absent: they live on a
-- different RDS instance (see header).
-- ─────────────────────────────────────────────────────────────────────────────

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_orgs'::regclass
      and conname = 'zed_orgs_created_by_fk'
  ) then
    alter table if exists zed_orgs
      add constraint zed_orgs_created_by_fk
      foreign key (created_by_user_id) references zed_users(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_org_members'::regclass
      and conname = 'zed_org_members_org_fk'
  ) then
    alter table if exists zed_org_members
      add constraint zed_org_members_org_fk
      foreign key (org_id) references zed_orgs(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_org_members'::regclass
      and conname = 'zed_org_members_user_fk'
  ) then
    alter table if exists zed_org_members
      add constraint zed_org_members_user_fk
      foreign key (user_id) references zed_users(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_org_invitations'::regclass
      and conname = 'zed_org_invitations_org_fk'
  ) then
    alter table if exists zed_org_invitations
      add constraint zed_org_invitations_org_fk
      foreign key (org_id) references zed_orgs(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_org_invitations'::regclass
      and conname = 'zed_org_invitations_invited_by_fk'
  ) then
    alter table if exists zed_org_invitations
      add constraint zed_org_invitations_invited_by_fk
      foreign key (invited_by_user_id) references zed_users(id) on delete restrict;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_org_invitations'::regclass
      and conname = 'zed_org_invitations_accepted_by_fk'
  ) then
    alter table if exists zed_org_invitations
      add constraint zed_org_invitations_accepted_by_fk
      foreign key (accepted_by_user_id) references zed_users(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_projects'::regclass
      and conname = 'zed_projects_org_fk'
  ) then
    alter table if exists zed_projects
      add constraint zed_projects_org_fk
      foreign key (org_id) references zed_orgs(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_projects'::regclass
      and conname = 'zed_projects_created_by_fk'
  ) then
    alter table if exists zed_projects
      add constraint zed_projects_created_by_fk
      foreign key (created_by_user_id) references zed_users(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_project_members'::regclass
      and conname = 'zed_project_members_project_fk'
  ) then
    alter table if exists zed_project_members
      add constraint zed_project_members_project_fk
      foreign key (project_id) references zed_projects(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_project_members'::regclass
      and conname = 'zed_project_members_user_fk'
  ) then
    alter table if exists zed_project_members
      add constraint zed_project_members_user_fk
      foreign key (user_id) references zed_users(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_project_invitations'::regclass
      and conname = 'zed_project_invitations_project_fk'
  ) then
    alter table if exists zed_project_invitations
      add constraint zed_project_invitations_project_fk
      foreign key (project_id) references zed_projects(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_project_invitations'::regclass
      and conname = 'zed_project_invitations_invited_by_fk'
  ) then
    alter table if exists zed_project_invitations
      add constraint zed_project_invitations_invited_by_fk
      foreign key (invited_by_user_id) references zed_users(id) on delete restrict;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_project_invitations'::regclass
      and conname = 'zed_project_invitations_accepted_by_fk'
  ) then
    alter table if exists zed_project_invitations
      add constraint zed_project_invitations_accepted_by_fk
      foreign key (accepted_by_user_id) references zed_users(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_packages'::regclass
      and conname = 'zed_packages_org_fk'
  ) then
    alter table if exists zed_packages
      add constraint zed_packages_org_fk
      foreign key (org_id) references zed_orgs(id) on delete cascade;
  end if;
end
$zed_fk$;

-- A package outlives the project it was filed under; dropping the project only
-- unfiles it, because the package URL is org-scoped.
do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_packages'::regclass
      and conname = 'zed_packages_project_fk'
  ) then
    alter table if exists zed_packages
      add constraint zed_packages_project_fk
      foreign key (project_id) references zed_projects(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_packages'::regclass
      and conname = 'zed_packages_created_by_fk'
  ) then
    alter table if exists zed_packages
      add constraint zed_packages_created_by_fk
      foreign key (created_by_user_id) references zed_users(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_versions'::regclass
      and conname = 'zed_package_versions_package_fk'
  ) then
    alter table if exists zed_package_versions
      add constraint zed_package_versions_package_fk
      foreign key (package_id) references zed_packages(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_versions'::regclass
      and conname = 'zed_package_versions_published_by_fk'
  ) then
    alter table if exists zed_package_versions
      add constraint zed_package_versions_published_by_fk
      foreign key (published_by_user_id) references zed_users(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_dependency_graph_artifacts'::regclass
      and conname = 'zed_dependency_graph_artifacts_root_version_fk'
  ) then
    alter table if exists zed_dependency_graph_artifacts
      add constraint zed_dependency_graph_artifacts_root_version_fk
      foreign key (root_package_version_id) references zed_package_versions(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_dependency_graph_edges'::regclass
      and conname = 'zed_dependency_graph_edges_artifact_fk'
  ) then
    alter table if exists zed_dependency_graph_edges
      add constraint zed_dependency_graph_edges_artifact_fk
      foreign key (graph_artifact_id) references zed_dependency_graph_artifacts(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_dependency_graph_edges'::regclass
      and conname = 'zed_dependency_graph_edges_from_package_fk'
  ) then
    alter table if exists zed_dependency_graph_edges
      add constraint zed_dependency_graph_edges_from_package_fk
      foreign key (from_package_id) references zed_packages(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_dependency_graph_edges'::regclass
      and conname = 'zed_dependency_graph_edges_from_version_fk'
  ) then
    alter table if exists zed_dependency_graph_edges
      add constraint zed_dependency_graph_edges_from_version_fk
      foreign key (from_package_version_id) references zed_package_versions(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_dependency_graph_edges'::regclass
      and conname = 'zed_dependency_graph_edges_to_package_fk'
  ) then
    alter table if exists zed_dependency_graph_edges
      add constraint zed_dependency_graph_edges_to_package_fk
      foreign key (to_package_id) references zed_packages(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_dependency_graph_edges'::regclass
      and conname = 'zed_dependency_graph_edges_to_version_fk'
  ) then
    alter table if exists zed_dependency_graph_edges
      add constraint zed_dependency_graph_edges_to_version_fk
      foreign key (to_package_version_id) references zed_package_versions(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_licenses'::regclass
      and conname = 'zed_package_licenses_package_fk'
  ) then
    alter table if exists zed_package_licenses
      add constraint zed_package_licenses_package_fk
      foreign key (package_id) references zed_packages(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_licenses'::regclass
      and conname = 'zed_package_licenses_version_fk'
  ) then
    alter table if exists zed_package_licenses
      add constraint zed_package_licenses_version_fk
      foreign key (package_version_id) references zed_package_versions(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_uploads'::regclass
      and conname = 'zed_package_uploads_package_fk'
  ) then
    alter table if exists zed_package_uploads
      add constraint zed_package_uploads_package_fk
      foreign key (package_id) references zed_packages(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_uploads'::regclass
      and conname = 'zed_package_uploads_version_fk'
  ) then
    alter table if exists zed_package_uploads
      add constraint zed_package_uploads_version_fk
      foreign key (package_version_id) references zed_package_versions(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_uploads'::regclass
      and conname = 'zed_package_uploads_user_fk'
  ) then
    alter table if exists zed_package_uploads
      add constraint zed_package_uploads_user_fk
      foreign key (uploaded_by_user_id) references zed_users(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_uploads'::regclass
      and conname = 'zed_package_uploads_token_fk'
  ) then
    alter table if exists zed_package_uploads
      add constraint zed_package_uploads_token_fk
      foreign key (api_token_id) references zed_api_tokens(id) on delete set null;
  end if;
end
$zed_fk$;

-- The download ledger keeps its rows when a package is removed only through
-- soft delete; a hard delete cascades, since the counter it feeds goes too.
do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_downloads'::regclass
      and conname = 'zed_package_downloads_package_fk'
  ) then
    alter table if exists zed_package_downloads
      add constraint zed_package_downloads_package_fk
      foreign key (package_id) references zed_packages(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_downloads'::regclass
      and conname = 'zed_package_downloads_version_fk'
  ) then
    alter table if exists zed_package_downloads
      add constraint zed_package_downloads_version_fk
      foreign key (package_version_id) references zed_package_versions(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_downloads'::regclass
      and conname = 'zed_package_downloads_user_fk'
  ) then
    alter table if exists zed_package_downloads
      add constraint zed_package_downloads_user_fk
      foreign key (downloaded_by_user_id) references zed_users(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_package_downloads'::regclass
      and conname = 'zed_package_downloads_token_fk'
  ) then
    alter table if exists zed_package_downloads
      add constraint zed_package_downloads_token_fk
      foreign key (api_token_id) references zed_api_tokens(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_entity_embeddings'::regclass
      and conname = 'zed_entity_embeddings_org_fk'
  ) then
    alter table if exists zed_entity_embeddings
      add constraint zed_entity_embeddings_org_fk
      foreign key (org_id) references zed_orgs(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_api_tokens'::regclass
      and conname = 'zed_api_tokens_org_fk'
  ) then
    alter table if exists zed_api_tokens
      add constraint zed_api_tokens_org_fk
      foreign key (org_id) references zed_orgs(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_api_tokens'::regclass
      and conname = 'zed_api_tokens_user_fk'
  ) then
    alter table if exists zed_api_tokens
      add constraint zed_api_tokens_user_fk
      foreign key (user_id) references zed_users(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_audit_log'::regclass
      and conname = 'zed_audit_log_org_fk'
  ) then
    alter table if exists zed_audit_log
      add constraint zed_audit_log_org_fk
      foreign key (org_id) references zed_orgs(id) on delete cascade;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_audit_log'::regclass
      and conname = 'zed_audit_log_actor_fk'
  ) then
    alter table if exists zed_audit_log
      add constraint zed_audit_log_actor_fk
      foreign key (actor_user_id) references zed_users(id) on delete set null;
  end if;
end
$zed_fk$;

do $zed_fk$
begin
  if not exists (
    select 1
    from pg_constraint
    where conrelid = 'zed_audit_log'::regclass
      and conname = 'zed_audit_log_token_fk'
  ) then
    alter table if exists zed_audit_log
      add constraint zed_audit_log_token_fk
      foreign key (api_token_id) references zed_api_tokens(id) on delete set null;
  end if;
end
$zed_fk$;
