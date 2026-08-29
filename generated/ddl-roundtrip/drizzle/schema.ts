// SHADOW ONLY. Generated from authored DDL; never use this file as migration authority.
import { pgTable, index, uniqueIndex, foreignKey, check, uuid, varchar, text, jsonb, boolean, timestamp, bigint, integer, primaryKey } from "drizzle-orm/pg-core"
import { sql } from "drizzle-orm"



export const zed_api_tokens = pgTable("zed_api_tokens", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	name: text().notNull(),
	token_hash: varchar({ length: 64 }).notNull(),
	org_id: uuid(),
	user_id: uuid(),
	role: varchar({ length: 16 }).default('publish').notNull(),
	last_used_at: timestamp({ withTimezone: true, mode: 'string' }),
	expires_at: timestamp({ withTimezone: true, mode: 'string' }),
	revoked_at: timestamp({ withTimezone: true, mode: 'string' }),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	updated_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	check("zed_api_tokens_name_size_chk", sql`(octet_length(name) >= 1) AND (octet_length(name) <= 200)`),
	foreignKey({
			columns: [table.org_id],
			foreignColumns: [zed_orgs.id],
			name: "zed_api_tokens_org_fk"
		}).onDelete("cascade"),
	index("zed_api_tokens_org_idx").using("btree", table.org_id.asc().nullsLast()).where(sql`(org_id IS NOT NULL)`),
	check("zed_api_tokens_role_chk", sql`(role)::text = ANY ((ARRAY['read'::character varying, 'publish'::character varying, 'admin'::character varying])::text[])`),
	check("zed_api_tokens_scope_chk", sql`(org_id IS NOT NULL) OR (user_id IS NOT NULL)`),
	check("zed_api_tokens_token_hash_chk", sql`(token_hash)::text ~ '^[a-f0-9]{64}$'::text`),
	uniqueIndex("zed_api_tokens_token_hash_uq").using("btree", table.token_hash.asc().nullsLast()),
	foreignKey({
			columns: [table.user_id],
			foreignColumns: [zed_users.id],
			name: "zed_api_tokens_user_fk"
		}).onDelete("cascade"),
	index("zed_api_tokens_user_idx").using("btree", table.user_id.asc().nullsLast()).where(sql`(user_id IS NOT NULL)`),
]);

export const zed_audit_log = pgTable("zed_audit_log", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	org_id: uuid(),
	actor_user_id: uuid(),
	api_token_id: uuid(),
	action: varchar({ length: 64 }).notNull(),
	entity_type: varchar({ length: 24 }).notNull(),
	entity_id: uuid(),
	detail: jsonb().default({}).notNull(),
	client_ip_hash: varchar({ length: 64 }),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	check("zed_audit_log_action_format_chk", sql`(action)::text ~ '^[a-z][a-z0-9_.]{0,63}$'::text`),
	foreignKey({
			columns: [table.actor_user_id],
			foreignColumns: [zed_users.id],
			name: "zed_audit_log_actor_fk"
		}).onDelete("set null"),
	index("zed_audit_log_actor_idx").using("btree", table.actor_user_id.asc().nullsLast(), table.created_at.desc().nullsFirst()).where(sql`(actor_user_id IS NOT NULL)`),
	check("zed_audit_log_detail_object_chk", sql`jsonb_typeof(detail) = 'object'::text`),
	index("zed_audit_log_entity_idx").using("btree", table.entity_type.asc().nullsLast(), table.entity_id.asc().nullsLast(), table.created_at.desc().nullsFirst()),
	check("zed_audit_log_entity_type_chk", sql`(entity_type)::text = ANY ((ARRAY['user'::character varying, 'org'::character varying, 'project'::character varying, 'package'::character varying, 'package_version'::character varying, 'package_license'::character varying, 'api_token'::character varying, 'invitation'::character varying])::text[])`),
	check("zed_audit_log_ip_hash_chk", sql`(client_ip_hash IS NULL) OR ((client_ip_hash)::text ~ '^[a-f0-9]{64}$'::text)`),
	foreignKey({
			columns: [table.org_id],
			foreignColumns: [zed_orgs.id],
			name: "zed_audit_log_org_fk"
		}).onDelete("cascade"),
	index("zed_audit_log_org_idx").using("btree", table.org_id.asc().nullsLast(), table.created_at.desc().nullsFirst()).where(sql`(org_id IS NOT NULL)`),
	foreignKey({
			columns: [table.api_token_id],
			foreignColumns: [zed_api_tokens.id],
			name: "zed_audit_log_token_fk"
		}).onDelete("set null"),
]);

export const zed_dependency_graph_artifacts = pgTable("zed_dependency_graph_artifacts", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	root_package_version_id: uuid().notNull(),
	graph_kind: varchar({ length: 16 }).notNull(),
	schema_version: varchar({ length: 96 }).notNull(),
	graph_digest: varchar({ length: 71 }).notNull(),
	resolver_name: varchar({ length: 120 }),
	resolver_version: varchar({ length: 120 }),
	resolution_input_digest: varchar({ length: 71 }),
	registry_checkpoint: text(),
	target: jsonb().default({}).notNull(),
	enabled_features: jsonb().default([]).notNull(),
	document: jsonb().notNull(),
	node_count: integer().notNull(),
	edge_count: integer().notNull(),
	max_depth: integer().notNull(),
	cycle_count: integer().default(0).notNull(),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	sealed_at: timestamp({ withTimezone: true, mode: 'string' }),
}, (table) => [
	check("zed_dependency_graph_artifacts_checkpoint_size_chk", sql`(registry_checkpoint IS NULL) OR (octet_length(registry_checkpoint) <= 1024)`),
	check("zed_dependency_graph_artifacts_counts_chk", sql`(node_count >= 1) AND (edge_count >= 0) AND (max_depth >= 0) AND (cycle_count >= 0)`),
	check("zed_dependency_graph_artifacts_declared_metadata_chk", sql`((graph_kind)::text <> 'declared'::text) OR ((registry_checkpoint IS NULL) AND (target = '{}'::jsonb) AND (enabled_features = '[]'::jsonb))`),
	uniqueIndex("zed_dependency_graph_artifacts_declared_root_uq").using("btree", table.root_package_version_id.asc().nullsLast()).where(sql`((graph_kind)::text = 'declared'::text)`),
	check("zed_dependency_graph_artifacts_default_limits_chk", sql`(node_count <= 50000) AND (edge_count <= 500000) AND (max_depth <= node_count)`),
	check("zed_dependency_graph_artifacts_digest_chk", sql`(graph_digest)::text ~ '^sha256:[a-f0-9]{64}$'::text`),
	uniqueIndex("zed_dependency_graph_artifacts_digest_uq").using("btree", table.graph_digest.asc().nullsLast()),
	check("zed_dependency_graph_artifacts_document_binding_chk", sql`(NOT ((document ->> 'schema'::text) IS DISTINCT FROM (schema_version)::text)) AND (NOT ((document ->> 'graph_digest'::text) IS DISTINCT FROM (graph_digest)::text)) AND (NOT ((document ->> 'view'::text) IS DISTINCT FROM (graph_kind)::text)) AND (((graph_kind)::text <> 'resolved'::text) OR (NOT ((document #>> '{provenance,resolver_version}'::text[]) IS DISTINCT FROM (resolver_version)::text)))`),
	check("zed_dependency_graph_artifacts_document_object_chk", sql`jsonb_typeof(document) = 'object'::text`),
	check("zed_dependency_graph_artifacts_features_array_chk", sql`jsonb_typeof(enabled_features) = 'array'::text`),
	check("zed_dependency_graph_artifacts_input_digest_chk", sql`(resolution_input_digest IS NULL) OR ((resolution_input_digest)::text ~ '^sha256:[a-f0-9]{64}$'::text)`),
	check("zed_dependency_graph_artifacts_kind_chk", sql`(graph_kind)::text = ANY ((ARRAY['declared'::character varying, 'resolved'::character varying])::text[])`),
	check("zed_dependency_graph_artifacts_resolution_shape_chk", sql`(((graph_kind)::text = 'declared'::text) AND (resolver_name IS NULL) AND (resolver_version IS NULL) AND (resolution_input_digest IS NULL)) OR (((graph_kind)::text = 'resolved'::text) AND (resolver_name IS NOT NULL) AND (resolver_version IS NOT NULL) AND (resolution_input_digest IS NOT NULL))`),
	check("zed_dependency_graph_artifacts_resolved_features_chk", sql`((graph_kind)::text <> 'resolved'::text) OR (enabled_features = COALESCE((document #> '{provenance,enabled_features}'::text[]), '[]'::jsonb))`),
	index("zed_dependency_graph_artifacts_resolved_input_idx").using("btree", table.resolution_input_digest.asc().nullsLast()).where(sql`((graph_kind)::text = 'resolved'::text)`),
	check("zed_dependency_graph_artifacts_resolver_name_size_chk", sql`(resolver_name IS NULL) OR ((octet_length((resolver_name)::text) >= 1) AND (octet_length((resolver_name)::text) <= 120))`),
	check("zed_dependency_graph_artifacts_resolver_version_size_chk", sql`(resolver_version IS NULL) OR ((octet_length((resolver_version)::text) >= 1) AND (octet_length((resolver_version)::text) <= 120))`),
	index("zed_dependency_graph_artifacts_root_created_idx").using("btree", table.root_package_version_id.asc().nullsLast(), table.created_at.desc().nullsFirst()),
	foreignKey({
			columns: [table.root_package_version_id],
			foreignColumns: [zed_package_versions.id],
			name: "zed_dependency_graph_artifacts_root_version_fk"
		}).onDelete("cascade"),
	check("zed_dependency_graph_artifacts_schema_size_chk", sql`(octet_length((schema_version)::text) >= 1) AND (octet_length((schema_version)::text) <= 96)`),
	check("zed_dependency_graph_artifacts_sealed_at_chk", sql`(sealed_at IS NULL) OR (sealed_at >= created_at)`),
	check("zed_dependency_graph_artifacts_target_object_chk", sql`jsonb_typeof(target) = 'object'::text`),
	index("zed_dependency_graph_artifacts_unsealed_idx").using("btree", table.created_at.asc().nullsLast()).where(sql`(sealed_at IS NULL)`),
]);

export const zed_dependency_graph_edges = pgTable("zed_dependency_graph_edges", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	graph_artifact_id: uuid().notNull(),
	ordinal: integer().notNull(),
	from_registry_id: text().notNull(),
	from_org_slug: varchar({ length: 64 }).notNull(),
	from_package_name: varchar({ length: 128 }).notNull(),
	from_version: varchar({ length: 128 }),
	from_package_id: uuid(),
	from_package_version_id: uuid(),
	to_registry_id: text().notNull(),
	to_org_slug: varchar({ length: 64 }).notNull(),
	to_package_name: varchar({ length: 128 }).notNull(),
	to_version: varchar({ length: 128 }),
	to_package_id: uuid(),
	to_package_version_id: uuid(),
	requirement: text(),
	dependency_kind: varchar({ length: 16 }).notNull(),
	optional: boolean().default(false).notNull(),
	default_features: boolean().default(true).notNull(),
	features: jsonb().default([]).notNull(),
	target: text(),
	minimum_depth: integer().notNull(),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	foreignKey({
			columns: [table.graph_artifact_id],
			foreignColumns: [zed_dependency_graph_artifacts.id],
			name: "zed_dependency_graph_edges_artifact_fk"
		}).onDelete("cascade"),
	uniqueIndex("zed_dependency_graph_edges_artifact_ordinal_uq").using("btree", table.graph_artifact_id.asc().nullsLast(), table.ordinal.asc().nullsLast()),
	check("zed_dependency_graph_edges_depth_chk", sql`minimum_depth >= 1`),
	check("zed_dependency_graph_edges_features_array_chk", sql`jsonb_typeof(features) = 'array'::text`),
	foreignKey({
			columns: [table.from_package_id],
			foreignColumns: [zed_packages.id],
			name: "zed_dependency_graph_edges_from_package_fk"
		}).onDelete("set null"),
	index("zed_dependency_graph_edges_from_package_idx").using("btree", table.from_package_id.asc().nullsLast(), table.graph_artifact_id.asc().nullsLast()).where(sql`(from_package_id IS NOT NULL)`),
	foreignKey({
			columns: [table.from_package_version_id],
			foreignColumns: [zed_package_versions.id],
			name: "zed_dependency_graph_edges_from_version_fk"
		}).onDelete("set null"),
	index("zed_dependency_graph_edges_from_version_idx").using("btree", table.from_package_version_id.asc().nullsLast(), table.graph_artifact_id.asc().nullsLast()).where(sql`(from_package_version_id IS NOT NULL)`),
	index("zed_dependency_graph_edges_incoming_idx").using("btree", table.to_registry_id.asc().nullsLast(), table.to_org_slug.asc().nullsLast(), table.to_package_name.asc().nullsLast(), table.minimum_depth.asc().nullsLast()),
	index("zed_dependency_graph_edges_incoming_version_idx").using("btree", table.to_registry_id.asc().nullsLast(), table.to_org_slug.asc().nullsLast(), table.to_package_name.asc().nullsLast(), table.to_version.asc().nullsLast(), table.minimum_depth.asc().nullsLast(), table.graph_artifact_id.asc().nullsLast(), table.ordinal.asc().nullsLast()).where(sql`(to_version IS NOT NULL)`),
	check("zed_dependency_graph_edges_kind_chk", sql`(dependency_kind)::text = ANY ((ARRAY['runtime'::character varying, 'build'::character varying, 'development'::character varying, 'peer'::character varying, 'tooling'::character varying])::text[])`),
	check("zed_dependency_graph_edges_ordinal_chk", sql`ordinal >= 0`),
	check("zed_dependency_graph_edges_org_format_chk", sql`((from_org_slug)::text ~ '^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$'::text) AND ((to_org_slug)::text ~ '^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$'::text)`),
	index("zed_dependency_graph_edges_outgoing_idx").using("btree", table.from_registry_id.asc().nullsLast(), table.from_org_slug.asc().nullsLast(), table.from_package_name.asc().nullsLast(), table.minimum_depth.asc().nullsLast()),
	index("zed_dependency_graph_edges_outgoing_version_idx").using("btree", table.from_registry_id.asc().nullsLast(), table.from_org_slug.asc().nullsLast(), table.from_package_name.asc().nullsLast(), table.from_version.asc().nullsLast(), table.minimum_depth.asc().nullsLast(), table.graph_artifact_id.asc().nullsLast(), table.ordinal.asc().nullsLast()).where(sql`(from_version IS NOT NULL)`),
	check("zed_dependency_graph_edges_package_format_chk", sql`((from_package_name)::text ~ '^[a-z0-9][a-z0-9._-]{0,126}[a-z0-9]$'::text) AND ((to_package_name)::text ~ '^[a-z0-9][a-z0-9._-]{0,126}[a-z0-9]$'::text)`),
	check("zed_dependency_graph_edges_registry_size_chk", sql`((octet_length(from_registry_id) >= 1) AND (octet_length(from_registry_id) <= 512)) AND ((octet_length(to_registry_id) >= 1) AND (octet_length(to_registry_id) <= 512))`),
	check("zed_dependency_graph_edges_requirement_size_chk", sql`(requirement IS NULL) OR ((octet_length(requirement) >= 1) AND (octet_length(requirement) <= 1024))`),
	check("zed_dependency_graph_edges_source_version_chk", sql`(from_package_version_id IS NULL) OR ((from_package_id IS NOT NULL) AND (from_version IS NOT NULL))`),
	check("zed_dependency_graph_edges_target_size_chk", sql`(target IS NULL) OR (octet_length(target) <= 512)`),
	check("zed_dependency_graph_edges_target_version_chk", sql`(to_package_version_id IS NULL) OR ((to_package_id IS NOT NULL) AND (to_version IS NOT NULL))`),
	foreignKey({
			columns: [table.to_package_id],
			foreignColumns: [zed_packages.id],
			name: "zed_dependency_graph_edges_to_package_fk"
		}).onDelete("set null"),
	index("zed_dependency_graph_edges_to_package_idx").using("btree", table.to_package_id.asc().nullsLast(), table.graph_artifact_id.asc().nullsLast()).where(sql`(to_package_id IS NOT NULL)`),
	foreignKey({
			columns: [table.to_package_version_id],
			foreignColumns: [zed_package_versions.id],
			name: "zed_dependency_graph_edges_to_version_fk"
		}).onDelete("set null"),
	index("zed_dependency_graph_edges_to_version_idx").using("btree", table.to_package_version_id.asc().nullsLast(), table.graph_artifact_id.asc().nullsLast()).where(sql`(to_package_version_id IS NOT NULL)`),
	index("zed_dependency_graph_edges_unresolved_target_idx").using("btree", table.to_registry_id.asc().nullsLast(), table.to_org_slug.asc().nullsLast(), table.to_package_name.asc().nullsLast()).where(sql`(to_package_version_id IS NULL)`),
	check("zed_dependency_graph_edges_version_size_chk", sql`((from_version IS NULL) OR ((octet_length((from_version)::text) >= 1) AND (octet_length((from_version)::text) <= 128))) AND ((to_version IS NULL) OR ((octet_length((to_version)::text) >= 1) AND (octet_length((to_version)::text) <= 128)))`),
]);

export const zed_entity_embeddings = pgTable("zed_entity_embeddings", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	entity_type: varchar({ length: 24 }).notNull(),
	entity_id: uuid().notNull(),
	org_id: uuid(),
	embedding_model: varchar({ length: 120 }).notNull(),
	embedding: jsonb().notNull(),
	embedding_dimensions: integer().notNull(),
	content_sha256: varchar({ length: 64 }).notNull(),
	content_preview: text(),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	updated_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	check("zed_entity_embeddings_array_chk", sql`jsonb_typeof(embedding) = 'array'::text`),
	check("zed_entity_embeddings_dimensions_chk", sql`embedding_dimensions > 0`),
	check("zed_entity_embeddings_dimensions_match_chk", sql`jsonb_array_length(embedding) = embedding_dimensions`),
	index("zed_entity_embeddings_entity_idx").using("btree", table.entity_type.asc().nullsLast(), table.entity_id.asc().nullsLast()),
	uniqueIndex("zed_entity_embeddings_entity_model_sha_uq").using("btree", table.entity_type.asc().nullsLast(), table.entity_id.asc().nullsLast(), table.embedding_model.asc().nullsLast(), table.content_sha256.asc().nullsLast()),
	check("zed_entity_embeddings_entity_type_chk", sql`(entity_type)::text = ANY ((ARRAY['org'::character varying, 'project'::character varying, 'package'::character varying, 'package_version'::character varying])::text[])`),
	check("zed_entity_embeddings_model_format_chk", sql`(embedding_model)::text ~ '^[A-Za-z0-9._:/-]{1,120}$'::text`),
	foreignKey({
			columns: [table.org_id],
			foreignColumns: [zed_orgs.id],
			name: "zed_entity_embeddings_org_fk"
		}).onDelete("cascade"),
	index("zed_entity_embeddings_org_model_idx").using("btree", table.org_id.asc().nullsLast(), table.embedding_model.asc().nullsLast()).where(sql`(org_id IS NOT NULL)`),
	check("zed_entity_embeddings_preview_size_chk", sql`(content_preview IS NULL) OR (octet_length(content_preview) <= 4096)`),
	check("zed_entity_embeddings_sha256_chk", sql`(content_sha256)::text ~ '^[a-f0-9]{64}$'::text`),
]);

export const zed_org_invitations = pgTable("zed_org_invitations", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	org_id: uuid().notNull(),
	invited_by_user_id: uuid().notNull(),
	email: text().notNull(),
	role: varchar({ length: 16 }).notNull(),
	token_hash: varchar({ length: 64 }).notNull(),
	expires_at: timestamp({ withTimezone: true, mode: 'string' }).notNull(),
	accepted_at: timestamp({ withTimezone: true, mode: 'string' }),
	accepted_by_user_id: uuid(),
	revoked_at: timestamp({ withTimezone: true, mode: 'string' }),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	foreignKey({
			columns: [table.accepted_by_user_id],
			foreignColumns: [zed_users.id],
			name: "zed_org_invitations_accepted_by_fk"
		}).onDelete("set null"),
	check("zed_org_invitations_accepted_chk", sql`(accepted_at IS NULL) = (accepted_by_user_id IS NULL)`),
	check("zed_org_invitations_email_size_chk", sql`(octet_length(email) >= 3) AND (octet_length(email) <= 320)`),
	foreignKey({
			columns: [table.invited_by_user_id],
			foreignColumns: [zed_users.id],
			name: "zed_org_invitations_invited_by_fk"
		}).onDelete("restrict"),
	foreignKey({
			columns: [table.org_id],
			foreignColumns: [zed_orgs.id],
			name: "zed_org_invitations_org_fk"
		}).onDelete("cascade"),
	index("zed_org_invitations_org_idx").using("btree", table.org_id.asc().nullsLast(), table.created_at.desc().nullsFirst()),
	uniqueIndex("zed_org_invitations_pending_uq").using("btree", sql`org_id`, sql`lower(email)`).where(sql`((accepted_at IS NULL) AND (revoked_at IS NULL))`),
	check("zed_org_invitations_role_chk", sql`(role)::text = ANY ((ARRAY['admin'::character varying, 'member'::character varying, 'reader'::character varying])::text[])`),
	check("zed_org_invitations_token_hash_chk", sql`(token_hash)::text ~ '^[a-f0-9]{64}$'::text`),
	uniqueIndex("zed_org_invitations_token_hash_uq").using("btree", table.token_hash.asc().nullsLast()),
]);

export const zed_org_members = pgTable("zed_org_members", {
	org_id: uuid().notNull(),
	user_id: uuid().notNull(),
	role: varchar({ length: 16 }).notNull(),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	updated_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	foreignKey({
			columns: [table.org_id],
			foreignColumns: [zed_orgs.id],
			name: "zed_org_members_org_fk"
		}).onDelete("cascade"),
	primaryKey({ columns: [table.org_id, table.user_id], name: "zed_org_members_pkey"}),
	check("zed_org_members_role_chk", sql`(role)::text = ANY ((ARRAY['owner'::character varying, 'admin'::character varying, 'member'::character varying, 'reader'::character varying])::text[])`),
	foreignKey({
			columns: [table.user_id],
			foreignColumns: [zed_users.id],
			name: "zed_org_members_user_fk"
		}).onDelete("cascade"),
	index("zed_org_members_user_idx").using("btree", table.user_id.asc().nullsLast(), table.org_id.asc().nullsLast()),
]);

export const zed_orgs = pgTable("zed_orgs", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	slug: varchar({ length: 64 }).notNull(),
	name: text().notNull(),
	description: text(),
	settings: jsonb().default({}).notNull(),
	created_by_user_id: uuid(),
	is_soft_deleted: boolean().default(false).notNull(),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	updated_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	foreignKey({
			columns: [table.created_by_user_id],
			foreignColumns: [zed_users.id],
			name: "zed_orgs_created_by_fk"
		}).onDelete("set null"),
	index("zed_orgs_created_by_idx").using("btree", table.created_by_user_id.asc().nullsLast()).where(sql`(created_by_user_id IS NOT NULL)`),
	check("zed_orgs_description_size_chk", sql`(description IS NULL) OR (octet_length(description) <= 4096)`),
	check("zed_orgs_name_size_chk", sql`(octet_length(name) >= 1) AND (octet_length(name) <= 200)`),
	check("zed_orgs_settings_object_chk", sql`jsonb_typeof(settings) = 'object'::text`),
	uniqueIndex("zed_orgs_slug_active_uq").using("btree", table.slug.asc().nullsLast()).where(sql`(is_soft_deleted = false)`),
	check("zed_orgs_slug_format_chk", sql`(slug)::text ~ '^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$'::text`),
]);

export const zed_package_downloads = pgTable("zed_package_downloads", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	package_id: uuid().notNull(),
	package_version_id: uuid(),
	downloaded_by_user_id: uuid(),
	api_token_id: uuid(),
	source: varchar({ length: 16 }).default('cli').notNull(),
	format: varchar({ length: 16 }),
	// You can use { mode: "bigint" } if numbers are exceeding js number limitations
	bytes_sent: bigint({ mode: "number" }),
	client_ip_hash: varchar({ length: 64 }),
	user_agent: text(),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	check("zed_package_downloads_bytes_chk", sql`(bytes_sent IS NULL) OR (bytes_sent >= 0)`),
	index("zed_package_downloads_created_at_idx").using("btree", table.created_at.desc().nullsFirst()),
	check("zed_package_downloads_format_chk", sql`(format IS NULL) OR ((format)::text = ANY ((ARRAY['tar.gz'::character varying, 'tar.zst'::character varying, 'zip'::character varying])::text[]))`),
	check("zed_package_downloads_ip_hash_chk", sql`(client_ip_hash IS NULL) OR ((client_ip_hash)::text ~ '^[a-f0-9]{64}$'::text)`),
	foreignKey({
			columns: [table.package_id],
			foreignColumns: [zed_packages.id],
			name: "zed_package_downloads_package_fk"
		}).onDelete("cascade"),
	index("zed_package_downloads_package_idx").using("btree", table.package_id.asc().nullsLast(), table.created_at.desc().nullsFirst()),
	check("zed_package_downloads_source_chk", sql`(source)::text = ANY ((ARRAY['cli'::character varying, 'web'::character varying, 'api'::character varying, 'mirror'::character varying, 'ci'::character varying])::text[])`),
	foreignKey({
			columns: [table.api_token_id],
			foreignColumns: [zed_api_tokens.id],
			name: "zed_package_downloads_token_fk"
		}).onDelete("set null"),
	check("zed_package_downloads_user_agent_size_chk", sql`(user_agent IS NULL) OR (octet_length(user_agent) <= 512)`),
	foreignKey({
			columns: [table.downloaded_by_user_id],
			foreignColumns: [zed_users.id],
			name: "zed_package_downloads_user_fk"
		}).onDelete("set null"),
	index("zed_package_downloads_user_idx").using("btree", table.downloaded_by_user_id.asc().nullsLast(), table.created_at.desc().nullsFirst()).where(sql`(downloaded_by_user_id IS NOT NULL)`),
	foreignKey({
			columns: [table.package_version_id],
			foreignColumns: [zed_package_versions.id],
			name: "zed_package_downloads_version_fk"
		}).onDelete("set null"),
	index("zed_package_downloads_version_idx").using("btree", table.package_version_id.asc().nullsLast(), table.created_at.desc().nullsFirst()).where(sql`(package_version_id IS NOT NULL)`),
]);

export const zed_package_licenses = pgTable("zed_package_licenses", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	package_id: uuid().notNull(),
	package_version_id: uuid(),
	kind: varchar({ length: 16 }).default('spdx').notNull(),
	spdx_id: varchar({ length: 120 }),
	name: text(),
	url: text(),
	text_body: text(),
	is_primary: boolean().default(true).notNull(),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	updated_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	check("zed_package_licenses_custom_body_chk", sql`((kind)::text <> 'custom'::text) OR (text_body IS NOT NULL) OR (url IS NOT NULL)`),
	check("zed_package_licenses_kind_chk", sql`(kind)::text = ANY ((ARRAY['spdx'::character varying, 'custom'::character varying, 'proprietary'::character varying])::text[])`),
	check("zed_package_licenses_name_size_chk", sql`(name IS NULL) OR (octet_length(name) <= 200)`),
	foreignKey({
			columns: [table.package_id],
			foreignColumns: [zed_packages.id],
			name: "zed_package_licenses_package_fk"
		}).onDelete("cascade"),
	index("zed_package_licenses_package_idx").using("btree", table.package_id.asc().nullsLast()),
	uniqueIndex("zed_package_licenses_package_primary_uq").using("btree", table.package_id.asc().nullsLast()).where(sql`((package_version_id IS NULL) AND is_primary)`),
	check("zed_package_licenses_spdx_id_chk", sql`(((kind)::text = 'spdx'::text) AND ((spdx_id)::text ~ '^[A-Za-z0-9.+()-]{1,120}$'::text)) OR (((kind)::text <> 'spdx'::text) AND (spdx_id IS NULL))`),
	index("zed_package_licenses_spdx_idx").using("btree", table.spdx_id.asc().nullsLast()).where(sql`(spdx_id IS NOT NULL)`),
	check("zed_package_licenses_text_size_chk", sql`(text_body IS NULL) OR (octet_length(text_body) <= 262144)`),
	check("zed_package_licenses_url_size_chk", sql`(url IS NULL) OR (octet_length(url) <= 2048)`),
	foreignKey({
			columns: [table.package_version_id],
			foreignColumns: [zed_package_versions.id],
			name: "zed_package_licenses_version_fk"
		}).onDelete("cascade"),
	index("zed_package_licenses_version_idx").using("btree", table.package_version_id.asc().nullsLast()).where(sql`(package_version_id IS NOT NULL)`),
	uniqueIndex("zed_package_licenses_version_primary_uq").using("btree", table.package_version_id.asc().nullsLast()).where(sql`((package_version_id IS NOT NULL) AND is_primary)`),
]);

export const zed_package_uploads = pgTable("zed_package_uploads", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	package_id: uuid().notNull(),
	package_version_id: uuid(),
	requested_version: varchar({ length: 128 }).notNull(),
	status: varchar({ length: 16 }).default('pending').notNull(),
	storage_backend: varchar({ length: 16 }).default('s3').notNull(),
	storage_key: text(),
	format: varchar({ length: 16 }),
	// You can use { mode: "bigint" } if numbers are exceeding js number limitations
	size_bytes: bigint({ mode: "number" }),
	sha256: varchar({ length: 64 }),
	uploaded_by_user_id: uuid(),
	api_token_id: uuid(),
	client_ip_hash: varchar({ length: 64 }),
	user_agent: text(),
	error: text(),
	started_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	completed_at: timestamp({ withTimezone: true, mode: 'string' }),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	updated_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	check("zed_package_uploads_backend_chk", sql`(storage_backend)::text = ANY ((ARRAY['s3'::character varying, 'r2'::character varying, 'gcs'::character varying, 'fs'::character varying])::text[])`),
	check("zed_package_uploads_completed_chk", sql`((status)::text <> ALL ((ARRAY['verified'::character varying, 'failed'::character varying, 'aborted'::character varying])::text[])) OR (completed_at IS NOT NULL)`),
	check("zed_package_uploads_failed_chk", sql`((status)::text <> ALL ((ARRAY['failed'::character varying, 'aborted'::character varying])::text[])) OR (package_version_id IS NULL)`),
	check("zed_package_uploads_format_chk", sql`(format IS NULL) OR ((format)::text = ANY ((ARRAY['tar.gz'::character varying, 'tar.zst'::character varying, 'zip'::character varying])::text[]))`),
	check("zed_package_uploads_ip_hash_chk", sql`(client_ip_hash IS NULL) OR ((client_ip_hash)::text ~ '^[a-f0-9]{64}$'::text)`),
	foreignKey({
			columns: [table.package_id],
			foreignColumns: [zed_packages.id],
			name: "zed_package_uploads_package_fk"
		}).onDelete("cascade"),
	index("zed_package_uploads_package_idx").using("btree", table.package_id.asc().nullsLast(), table.started_at.desc().nullsFirst()),
	check("zed_package_uploads_sha256_chk", sql`(sha256 IS NULL) OR ((sha256)::text ~ '^[a-f0-9]{64}$'::text)`),
	check("zed_package_uploads_size_chk", sql`(size_bytes IS NULL) OR (size_bytes >= 0)`),
	check("zed_package_uploads_status_chk", sql`(status)::text = ANY ((ARRAY['pending'::character varying, 'uploading'::character varying, 'stored'::character varying, 'verified'::character varying, 'failed'::character varying, 'aborted'::character varying])::text[])`),
	index("zed_package_uploads_status_idx").using("btree", table.status.asc().nullsLast(), table.started_at.desc().nullsFirst()),
	check("zed_package_uploads_storage_key_size_chk", sql`(storage_key IS NULL) OR ((octet_length(storage_key) >= 1) AND (octet_length(storage_key) <= 1024))`),
	foreignKey({
			columns: [table.api_token_id],
			foreignColumns: [zed_api_tokens.id],
			name: "zed_package_uploads_token_fk"
		}).onDelete("set null"),
	check("zed_package_uploads_user_agent_size_chk", sql`(user_agent IS NULL) OR (octet_length(user_agent) <= 512)`),
	foreignKey({
			columns: [table.uploaded_by_user_id],
			foreignColumns: [zed_users.id],
			name: "zed_package_uploads_user_fk"
		}).onDelete("set null"),
	index("zed_package_uploads_user_idx").using("btree", table.uploaded_by_user_id.asc().nullsLast(), table.started_at.desc().nullsFirst()).where(sql`(uploaded_by_user_id IS NOT NULL)`),
	check("zed_package_uploads_verified_chk", sql`((status)::text <> 'verified'::text) OR (package_version_id IS NOT NULL)`),
	foreignKey({
			columns: [table.package_version_id],
			foreignColumns: [zed_package_versions.id],
			name: "zed_package_uploads_version_fk"
		}).onDelete("set null"),
	index("zed_package_uploads_version_idx").using("btree", table.package_version_id.asc().nullsLast()).where(sql`(package_version_id IS NOT NULL)`),
]);

export const zed_package_versions = pgTable("zed_package_versions", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	package_id: uuid().notNull(),
	version: varchar({ length: 128 }).notNull(),
	version_scheme: varchar({ length: 16 }).default('semver').notNull(),
	sha256: varchar({ length: 64 }).notNull(),
	// You can use { mode: "bigint" } if numbers are exceeding js number limitations
	size_bytes: bigint({ mode: "number" }).notNull(),
	format: varchar({ length: 16 }).notNull(),
	vcs_tag: varchar({ length: 160 }),
	vcs_commit: varchar({ length: 120 }),
	artifact_key: text().notNull(),
	manifest: jsonb().default({}).notNull(),
	// You can use { mode: "bigint" } if numbers are exceeding js number limitations
	download_count: bigint({ mode: "number" }).default(0).notNull(),
	yanked: boolean().default(false).notNull(),
	yanked_at: timestamp({ withTimezone: true, mode: 'string' }),
	yanked_reason: text(),
	published_by_user_id: uuid(),
	published_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	check("zed_package_versions_artifact_key_size_chk", sql`(octet_length(artifact_key) >= 1) AND (octet_length(artifact_key) <= 1024)`),
	check("zed_package_versions_download_count_chk", sql`download_count >= 0`),
	check("zed_package_versions_format_chk", sql`(format)::text = ANY ((ARRAY['tar.gz'::character varying, 'tar.zst'::character varying, 'zip'::character varying])::text[])`),
	check("zed_package_versions_manifest_object_chk", sql`jsonb_typeof(manifest) = 'object'::text`),
	foreignKey({
			columns: [table.package_id],
			foreignColumns: [zed_packages.id],
			name: "zed_package_versions_package_fk"
		}).onDelete("cascade"),
	index("zed_package_versions_package_published_idx").using("btree", table.package_id.asc().nullsLast(), table.published_at.desc().nullsFirst()),
	uniqueIndex("zed_package_versions_package_version_uq").using("btree", table.package_id.asc().nullsLast(), table.version.asc().nullsLast()),
	foreignKey({
			columns: [table.published_by_user_id],
			foreignColumns: [zed_users.id],
			name: "zed_package_versions_published_by_fk"
		}).onDelete("set null"),
	check("zed_package_versions_scheme_chk", sql`(version_scheme)::text = ANY ((ARRAY['semver'::character varying, 'calver'::character varying, 'opaque'::character varying])::text[])`),
	check("zed_package_versions_sha256_chk", sql`(sha256)::text ~ '^[a-f0-9]{64}$'::text`),
	index("zed_package_versions_sha256_idx").using("btree", table.sha256.asc().nullsLast()),
	check("zed_package_versions_size_chk", sql`size_bytes >= 0`),
	check("zed_package_versions_version_size_chk", sql`(octet_length((version)::text) >= 1) AND (octet_length((version)::text) <= 128)`),
	check("zed_package_versions_yanked_chk", sql`yanked = (yanked_at IS NOT NULL)`),
]);

export const zed_packages = pgTable("zed_packages", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	org_id: uuid().notNull(),
	project_id: uuid(),
	name: varchar({ length: 128 }).notNull(),
	description: text(),
	visibility: varchar({ length: 16 }).default('private').notNull(),
	vcs: varchar({ length: 16 }).default('git').notNull(),
	repo_url: text().default('').notNull(),
	homepage_url: text(),
	keywords: jsonb().default([]).notNull(),
	config: jsonb().default({}).notNull(),
	// You can use { mode: "bigint" } if numbers are exceeding js number limitations
	download_count: bigint({ mode: "number" }).default(0).notNull(),
	version_count: integer().default(0).notNull(),
	latest_version: varchar({ length: 128 }),
	first_published_at: timestamp({ withTimezone: true, mode: 'string' }),
	visibility_changed_at: timestamp({ withTimezone: true, mode: 'string' }),
	created_by_user_id: uuid(),
	is_soft_deleted: boolean().default(false).notNull(),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	updated_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	check("zed_packages_config_object_chk", sql`jsonb_typeof(config) = 'object'::text`),
	foreignKey({
			columns: [table.created_by_user_id],
			foreignColumns: [zed_users.id],
			name: "zed_packages_created_by_fk"
		}).onDelete("set null"),
	check("zed_packages_description_size_chk", sql`(description IS NULL) OR (octet_length(description) <= 4096)`),
	check("zed_packages_download_count_chk", sql`download_count >= 0`),
	index("zed_packages_download_count_idx").using("btree", table.download_count.desc().nullsFirst()).where(sql`(is_soft_deleted = false)`),
	check("zed_packages_homepage_url_size_chk", sql`(homepage_url IS NULL) OR (octet_length(homepage_url) <= 2048)`),
	check("zed_packages_keywords_array_chk", sql`jsonb_typeof(keywords) = 'array'::text`),
	check("zed_packages_name_format_chk", sql`(name)::text ~ '^[a-z0-9][a-z0-9._-]{0,126}[a-z0-9]$'::text`),
	foreignKey({
			columns: [table.org_id],
			foreignColumns: [zed_orgs.id],
			name: "zed_packages_org_fk"
		}).onDelete("cascade"),
	uniqueIndex("zed_packages_org_name_active_uq").using("btree", table.org_id.asc().nullsLast(), table.name.asc().nullsLast()).where(sql`(is_soft_deleted = false)`),
	foreignKey({
			columns: [table.project_id],
			foreignColumns: [zed_projects.id],
			name: "zed_packages_project_fk"
		}).onDelete("set null"),
	index("zed_packages_project_idx").using("btree", table.project_id.asc().nullsLast()).where(sql`(project_id IS NOT NULL)`),
	index("zed_packages_public_recent_idx").using("btree", table.created_at.desc().nullsFirst()).where(sql`(((visibility)::text = 'public'::text) AND (is_soft_deleted = false))`),
	check("zed_packages_repo_url_size_chk", sql`octet_length(repo_url) <= 2048`),
	check("zed_packages_vcs_chk", sql`(vcs)::text = ANY ((ARRAY['git'::character varying, 'hg'::character varying, 'svn'::character varying, 'fossil'::character varying])::text[])`),
	check("zed_packages_version_count_chk", sql`version_count >= 0`),
	check("zed_packages_visibility_chk", sql`(visibility)::text = ANY ((ARRAY['private'::character varying, 'internal'::character varying, 'public'::character varying])::text[])`),
]);

export const zed_project_invitations = pgTable("zed_project_invitations", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	project_id: uuid().notNull(),
	invited_by_user_id: uuid().notNull(),
	email: text().notNull(),
	role: varchar({ length: 16 }).notNull(),
	token_hash: varchar({ length: 64 }).notNull(),
	expires_at: timestamp({ withTimezone: true, mode: 'string' }).notNull(),
	accepted_at: timestamp({ withTimezone: true, mode: 'string' }),
	accepted_by_user_id: uuid(),
	revoked_at: timestamp({ withTimezone: true, mode: 'string' }),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	foreignKey({
			columns: [table.accepted_by_user_id],
			foreignColumns: [zed_users.id],
			name: "zed_project_invitations_accepted_by_fk"
		}).onDelete("set null"),
	check("zed_project_invitations_accepted_chk", sql`(accepted_at IS NULL) = (accepted_by_user_id IS NULL)`),
	check("zed_project_invitations_email_size_chk", sql`(octet_length(email) >= 3) AND (octet_length(email) <= 320)`),
	foreignKey({
			columns: [table.invited_by_user_id],
			foreignColumns: [zed_users.id],
			name: "zed_project_invitations_invited_by_fk"
		}).onDelete("restrict"),
	uniqueIndex("zed_project_invitations_pending_uq").using("btree", sql`project_id`, sql`lower(email)`).where(sql`((accepted_at IS NULL) AND (revoked_at IS NULL))`),
	foreignKey({
			columns: [table.project_id],
			foreignColumns: [zed_projects.id],
			name: "zed_project_invitations_project_fk"
		}).onDelete("cascade"),
	index("zed_project_invitations_project_idx").using("btree", table.project_id.asc().nullsLast(), table.created_at.desc().nullsFirst()),
	check("zed_project_invitations_role_chk", sql`(role)::text = ANY ((ARRAY['admin'::character varying, 'member'::character varying, 'reader'::character varying])::text[])`),
	check("zed_project_invitations_token_hash_chk", sql`(token_hash)::text ~ '^[a-f0-9]{64}$'::text`),
	uniqueIndex("zed_project_invitations_token_hash_uq").using("btree", table.token_hash.asc().nullsLast()),
]);

export const zed_project_members = pgTable("zed_project_members", {
	project_id: uuid().notNull(),
	user_id: uuid().notNull(),
	role: varchar({ length: 16 }).notNull(),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	updated_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	primaryKey({ columns: [table.project_id, table.user_id], name: "zed_project_members_pkey"}),
	foreignKey({
			columns: [table.project_id],
			foreignColumns: [zed_projects.id],
			name: "zed_project_members_project_fk"
		}).onDelete("cascade"),
	check("zed_project_members_role_chk", sql`(role)::text = ANY ((ARRAY['owner'::character varying, 'admin'::character varying, 'member'::character varying, 'reader'::character varying])::text[])`),
	foreignKey({
			columns: [table.user_id],
			foreignColumns: [zed_users.id],
			name: "zed_project_members_user_fk"
		}).onDelete("cascade"),
	index("zed_project_members_user_idx").using("btree", table.user_id.asc().nullsLast(), table.project_id.asc().nullsLast()),
]);

export const zed_projects = pgTable("zed_projects", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	org_id: uuid().notNull(),
	slug: varchar({ length: 64 }).notNull(),
	name: text().notNull(),
	description: text(),
	visibility: varchar({ length: 16 }).default('private').notNull(),
	settings: jsonb().default({}).notNull(),
	created_by_user_id: uuid(),
	is_soft_deleted: boolean().default(false).notNull(),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	updated_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	foreignKey({
			columns: [table.created_by_user_id],
			foreignColumns: [zed_users.id],
			name: "zed_projects_created_by_fk"
		}).onDelete("set null"),
	check("zed_projects_description_size_chk", sql`(description IS NULL) OR (octet_length(description) <= 4096)`),
	check("zed_projects_name_size_chk", sql`(octet_length(name) >= 1) AND (octet_length(name) <= 200)`),
	foreignKey({
			columns: [table.org_id],
			foreignColumns: [zed_orgs.id],
			name: "zed_projects_org_fk"
		}).onDelete("cascade"),
	index("zed_projects_org_idx").using("btree", table.org_id.asc().nullsLast(), table.updated_at.desc().nullsFirst()),
	uniqueIndex("zed_projects_org_slug_active_uq").using("btree", table.org_id.asc().nullsLast(), table.slug.asc().nullsLast()).where(sql`(is_soft_deleted = false)`),
	check("zed_projects_settings_object_chk", sql`jsonb_typeof(settings) = 'object'::text`),
	check("zed_projects_slug_format_chk", sql`(slug)::text ~ '^[a-z0-9][a-z0-9._-]{0,62}[a-z0-9]$'::text`),
	check("zed_projects_visibility_chk", sql`(visibility)::text = ANY ((ARRAY['private'::character varying, 'internal'::character varying, 'public'::character varying])::text[])`),
]);

export const zed_users = pgTable("zed_users", {
	id: uuid().defaultRandom().primaryKey().notNull(),
	shared_auth_subject: uuid().notNull(),
	auth_realm: varchar({ length: 16 }).default('customer').notNull(),
	email: text(),
	display_name: text(),
	avatar_url: text(),
	settings: jsonb().default({}).notNull(),
	is_soft_deleted: boolean().default(false).notNull(),
	created_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	updated_at: timestamp({ withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	check("zed_users_auth_realm_chk", sql`(auth_realm)::text = ANY ((ARRAY['customer'::character varying, 'admin'::character varying])::text[])`),
	check("zed_users_avatar_url_size_chk", sql`(avatar_url IS NULL) OR (octet_length(avatar_url) <= 2048)`),
	check("zed_users_display_name_size_chk", sql`(display_name IS NULL) OR (octet_length(display_name) <= 200)`),
	uniqueIndex("zed_users_email_active_uq").using("btree", sql`lower(email)`).where(sql`((email IS NOT NULL) AND (is_soft_deleted = false))`),
	check("zed_users_email_size_chk", sql`(email IS NULL) OR ((octet_length(email) >= 3) AND (octet_length(email) <= 320))`),
	check("zed_users_settings_object_chk", sql`jsonb_typeof(settings) = 'object'::text`),
	uniqueIndex("zed_users_subject_realm_uq").using("btree", table.auth_realm.asc().nullsLast(), table.shared_auth_subject.asc().nullsLast()),
]);
