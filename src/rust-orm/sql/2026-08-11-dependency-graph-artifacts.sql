-- Forward-only migration for registries that already recorded the historical
-- base schema ledger entry before dependency-graph persistence was added.
--
-- Canonical desired state remains in ../registry.sql. Every create is
-- idempotent, and foreign keys are guarded explicitly so an interrupted or
-- retried migration cannot replay non-idempotent ADD CONSTRAINT statements.

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
  constraint zed_dependency_graph_artifacts_counts_chk
    check (node_count >= 1 and edge_count >= 0 and max_depth >= 0 and cycle_count >= 0),
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

create index if not exists zed_dependency_graph_edges_from_package_idx
  on zed_dependency_graph_edges (from_package_id, graph_artifact_id)
  where from_package_id is not null;

create index if not exists zed_dependency_graph_edges_to_package_idx
  on zed_dependency_graph_edges (to_package_id, graph_artifact_id)
  where to_package_id is not null;

create index if not exists zed_dependency_graph_edges_to_version_idx
  on zed_dependency_graph_edges (to_package_version_id, graph_artifact_id)
  where to_package_version_id is not null;

create index if not exists zed_dependency_graph_edges_unresolved_target_idx
  on zed_dependency_graph_edges (to_registry_id, to_org_slug, to_package_name)
  where to_package_version_id is null;

do $zed_graph_constraints$
begin
  if not exists (
    select 1 from pg_constraint
     where conname = 'zed_dependency_graph_artifacts_root_version_fk'
       and conrelid = 'zed_dependency_graph_artifacts'::regclass
  ) then
    alter table zed_dependency_graph_artifacts
      add constraint zed_dependency_graph_artifacts_root_version_fk
      foreign key (root_package_version_id) references zed_package_versions(id) on delete cascade;
  end if;

  if not exists (
    select 1 from pg_constraint
     where conname = 'zed_dependency_graph_edges_artifact_fk'
       and conrelid = 'zed_dependency_graph_edges'::regclass
  ) then
    alter table zed_dependency_graph_edges
      add constraint zed_dependency_graph_edges_artifact_fk
      foreign key (graph_artifact_id) references zed_dependency_graph_artifacts(id) on delete cascade;
  end if;

  if not exists (
    select 1 from pg_constraint
     where conname = 'zed_dependency_graph_edges_from_package_fk'
       and conrelid = 'zed_dependency_graph_edges'::regclass
  ) then
    alter table zed_dependency_graph_edges
      add constraint zed_dependency_graph_edges_from_package_fk
      foreign key (from_package_id) references zed_packages(id) on delete set null;
  end if;

  if not exists (
    select 1 from pg_constraint
     where conname = 'zed_dependency_graph_edges_from_version_fk'
       and conrelid = 'zed_dependency_graph_edges'::regclass
  ) then
    alter table zed_dependency_graph_edges
      add constraint zed_dependency_graph_edges_from_version_fk
      foreign key (from_package_version_id) references zed_package_versions(id) on delete set null;
  end if;

  if not exists (
    select 1 from pg_constraint
     where conname = 'zed_dependency_graph_edges_to_package_fk'
       and conrelid = 'zed_dependency_graph_edges'::regclass
  ) then
    alter table zed_dependency_graph_edges
      add constraint zed_dependency_graph_edges_to_package_fk
      foreign key (to_package_id) references zed_packages(id) on delete set null;
  end if;

  if not exists (
    select 1 from pg_constraint
     where conname = 'zed_dependency_graph_edges_to_version_fk'
       and conrelid = 'zed_dependency_graph_edges'::regclass
  ) then
    alter table zed_dependency_graph_edges
      add constraint zed_dependency_graph_edges_to_version_fk
      foreign key (to_package_version_id) references zed_package_versions(id) on delete set null;
  end if;
end
$zed_graph_constraints$;
