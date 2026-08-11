-- Forward-only migration for registries that recorded the historical base
-- schema ledger entry before dependency-graph persistence was added.
--
-- Canonical desired state remains in ../registry.sql. This migration repeats
-- only the additive graph slice and is safe to retry: tables and indexes use
-- IF NOT EXISTS, functions are replaced, triggers are dropped and recreated,
-- and every foreign key is catalog-guarded.

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
