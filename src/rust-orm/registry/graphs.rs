//! Immutable dependency-graph documents and their normalized edge index.
//!
//! The JSON document is the lossless serialization authority. Edge rows are
//! derived query accelerators for reverse-impact and neighborhood reads; the
//! write path commits both representations atomically and rejects divergent
//! replays of an existing semantic digest.

use sea_orm::{
    prelude::Uuid, ColumnTrait, Condition, ConnectionTrait, EntityTrait, JoinType, QueryFilter,
    QueryOrder, QuerySelect, RelationTrait, Select,
};

#[cfg(feature = "read-write")]
use sea_orm::{
    prelude::Json, ActiveModelTrait, ActiveValue::Set, Statement, TransactionTrait, Value,
};

use crate::{
    entities::{dependency_graph_artifact, dependency_graph_edge, org, package, package_version},
    OrmError, ReadContext,
};

#[cfg(feature = "read-write")]
use crate::WriteContext;

#[cfg(feature = "read-write")]
use super::validation::{one_of, optional_text};

const GRAPH_KINDS: &[&str] = &["declared", "resolved"];
#[cfg(feature = "read-write")]
const DEPENDENCY_KINDS: &[&str] = &["runtime", "build", "development", "peer", "tooling"];

/// Maximum reverse-impact rows returned by one unpaginated call.
pub const GRAPH_EDGE_PAGE_LIMIT: u64 = 1_000;

/// One immutable graph document and the normalized edge index derived from it.
#[derive(Clone, Debug, PartialEq)]
pub struct DependencyGraphSnapshot {
    pub artifact: dependency_graph_artifact::Model,
    pub edges: Vec<dependency_graph_edge::Model>,
}

/// Registry coordinate used for incoming dependency queries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyGraphCoordinate {
    pub registry_id: String,
    pub org_slug: String,
    pub package_name: String,
    /// When present, restrict reverse impact to edges resolved to this spelling.
    pub version: Option<String>,
}

/// Return one graph by semantic digest, provided its root package is visible.
///
/// Public packages are always visible. The caller supplies organizations for
/// which private package visibility has already been authorized.
pub async fn dependency_graph_by_digest(
    context: &ReadContext,
    graph_digest: &str,
    visible_org_ids: &[Uuid],
) -> Result<Option<DependencyGraphSnapshot>, OrmError> {
    validate_prefixed_sha256("graph digest", graph_digest)?;
    let artifact = visible_artifacts(visible_org_ids)
        .filter(dependency_graph_artifact::Column::GraphDigest.eq(graph_digest))
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;
    match artifact {
        Some(artifact) => load_snapshot(context.connection(), artifact)
            .await
            .map(Some),
        None => Ok(None),
    }
}

/// Return the newest visible graph of `graph_kind` for a package version.
///
/// Declared graphs are unique per root version. Resolved graphs may have many
/// target/feature/checkpoint variants, so this convenience read selects the
/// newest; callers that need an exact resolution use its semantic digest.
pub async fn latest_dependency_graph_for_root(
    context: &ReadContext,
    root_package_version_id: Uuid,
    graph_kind: &str,
    visible_org_ids: &[Uuid],
) -> Result<Option<DependencyGraphSnapshot>, OrmError> {
    validate_graph_kind(graph_kind)?;
    let artifact = visible_artifacts(visible_org_ids)
        .filter(dependency_graph_artifact::Column::RootPackageVersionId.eq(root_package_version_id))
        .filter(dependency_graph_artifact::Column::GraphKind.eq(graph_kind))
        .order_by_desc(dependency_graph_artifact::Column::CreatedAt)
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;
    match artifact {
        Some(artifact) => load_snapshot(context.connection(), artifact)
            .await
            .map(Some),
        None => Ok(None),
    }
}

/// Visible graph edges which point at a registry package coordinate.
///
/// This is the canonical reverse-impact primitive. Visibility is determined by
/// each graph's root package; a private consumer graph is never leaked merely
/// because its dependency target is public.
pub async fn incoming_dependency_edges(
    context: &ReadContext,
    coordinate: &DependencyGraphCoordinate,
    visible_org_ids: &[Uuid],
    limit: u64,
) -> Result<Vec<dependency_graph_edge::Model>, OrmError> {
    validate_coordinate(coordinate)?;
    let mut query = visible_edges(visible_org_ids)
        .filter(dependency_graph_edge::Column::ToRegistryId.eq(coordinate.registry_id.as_str()))
        .filter(dependency_graph_edge::Column::ToOrgSlug.eq(coordinate.org_slug.as_str()))
        .filter(dependency_graph_edge::Column::ToPackageName.eq(coordinate.package_name.as_str()));
    if let Some(version) = coordinate.version.as_deref() {
        query = query.filter(dependency_graph_edge::Column::ToVersion.eq(version));
    }
    query
        .order_by_asc(dependency_graph_edge::Column::MinimumDepth)
        .order_by_asc(dependency_graph_edge::Column::GraphArtifactId)
        .order_by_asc(dependency_graph_edge::Column::Ordinal)
        .limit(limit.min(GRAPH_EDGE_PAGE_LIMIT))
        .all(context.connection())
        .await
        .map_err(OrmError::from_db_err)
}

fn visible_artifacts(visible_org_ids: &[Uuid]) -> Select<dependency_graph_artifact::Entity> {
    dependency_graph_artifact::Entity::find()
        .join(
            JoinType::InnerJoin,
            dependency_graph_artifact::Relation::RootPackageVersion.def(),
        )
        .join(
            JoinType::InnerJoin,
            package_version::Relation::Package.def(),
        )
        .join(JoinType::InnerJoin, package::Relation::Org.def())
        .filter(package::Column::IsSoftDeleted.eq(false))
        .filter(org::Column::IsSoftDeleted.eq(false))
        .filter(visibility_condition(visible_org_ids))
}

fn visible_edges(visible_org_ids: &[Uuid]) -> Select<dependency_graph_edge::Entity> {
    dependency_graph_edge::Entity::find()
        .join(
            JoinType::InnerJoin,
            dependency_graph_edge::Relation::GraphArtifact.def(),
        )
        .join(
            JoinType::InnerJoin,
            dependency_graph_artifact::Relation::RootPackageVersion.def(),
        )
        .join(
            JoinType::InnerJoin,
            package_version::Relation::Package.def(),
        )
        .join(JoinType::InnerJoin, package::Relation::Org.def())
        .filter(package::Column::IsSoftDeleted.eq(false))
        .filter(org::Column::IsSoftDeleted.eq(false))
        .filter(visibility_condition(visible_org_ids))
}

fn visibility_condition(visible_org_ids: &[Uuid]) -> Condition {
    let mut visibility = Condition::any().add(package::Column::Visibility.eq("public"));
    if !visible_org_ids.is_empty() {
        visibility = visibility.add(package::Column::OrgId.is_in(visible_org_ids.to_vec()));
    }
    visibility
}

async fn load_snapshot<C>(
    connection: &C,
    artifact: dependency_graph_artifact::Model,
) -> Result<DependencyGraphSnapshot, OrmError>
where
    C: ConnectionTrait,
{
    let edges = load_edges(connection, artifact.id).await?;
    Ok(DependencyGraphSnapshot { artifact, edges })
}

async fn load_edges<C>(
    connection: &C,
    graph_artifact_id: Uuid,
) -> Result<Vec<dependency_graph_edge::Model>, OrmError>
where
    C: ConnectionTrait,
{
    dependency_graph_edge::Entity::find()
        .filter(dependency_graph_edge::Column::GraphArtifactId.eq(graph_artifact_id))
        .order_by_asc(dependency_graph_edge::Column::Ordinal)
        .all(connection)
        .await
        .map_err(OrmError::from_db_err)
}

fn validate_graph_kind(graph_kind: &str) -> Result<(), OrmError> {
    if GRAPH_KINDS.contains(&graph_kind) {
        Ok(())
    } else {
        Err(OrmError::policy("graph kind must be declared or resolved"))
    }
}

fn validate_prefixed_sha256(field: &str, value: &str) -> Result<(), OrmError> {
    let digest = value.strip_prefix("sha256:");
    if digest.is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    }) {
        Ok(())
    } else {
        Err(OrmError::policy(format!(
            "{field} must be sha256: followed by 64 lowercase hexadecimal characters"
        )))
    }
}

fn validate_coordinate(coordinate: &DependencyGraphCoordinate) -> Result<(), OrmError> {
    validate_required_length("registry id", &coordinate.registry_id, 512)?;
    validate_org_slug("organization slug", &coordinate.org_slug)?;
    validate_package_name("package name", &coordinate.package_name)?;
    validate_optional_nonempty("package version", coordinate.version.as_deref(), 128)
}

fn validate_required_length(field: &str, value: &str, maximum: usize) -> Result<(), OrmError> {
    if value.trim().is_empty() || value.len() > maximum {
        Err(OrmError::policy(format!(
            "{field} must contain between 1 and {maximum} UTF-8 bytes"
        )))
    } else {
        Ok(())
    }
}

fn validate_optional_nonempty(
    field: &str,
    value: Option<&str>,
    maximum: usize,
) -> Result<(), OrmError> {
    match value {
        Some(value) => validate_required_length(field, value, maximum),
        None => Ok(()),
    }
}

fn validate_org_slug(field: &str, value: &str) -> Result<(), OrmError> {
    let bytes = value.as_bytes();
    if (2..=64).contains(&bytes.len())
        && is_ascii_lowercase_or_digit(bytes[0])
        && is_ascii_lowercase_or_digit(bytes[bytes.len() - 1])
        && bytes
            .iter()
            .all(|byte| is_ascii_lowercase_or_digit(*byte) || *byte == b'-')
    {
        Ok(())
    } else {
        Err(OrmError::policy(format!(
            "{field} must match the canonical lowercase registry slug format"
        )))
    }
}

fn validate_package_name(field: &str, value: &str) -> Result<(), OrmError> {
    let bytes = value.as_bytes();
    if (2..=128).contains(&bytes.len())
        && is_ascii_lowercase_or_digit(bytes[0])
        && is_ascii_lowercase_or_digit(bytes[bytes.len() - 1])
        && bytes
            .iter()
            .all(|byte| is_ascii_lowercase_or_digit(*byte) || matches!(*byte, b'.' | b'_' | b'-'))
    {
        Ok(())
    } else {
        Err(OrmError::policy(format!(
            "{field} must match the canonical lowercase package-name format"
        )))
    }
}

fn is_ascii_lowercase_or_digit(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit()
}

#[cfg(feature = "read-write")]
#[derive(Clone, Debug, PartialEq)]
pub struct DependencyGraphArtifactInput {
    pub root_package_version_id: Uuid,
    pub graph_kind: String,
    pub schema_version: String,
    pub graph_digest: String,
    pub resolver_name: Option<String>,
    pub resolver_version: Option<String>,
    pub resolution_input_digest: Option<String>,
    pub registry_checkpoint: Option<String>,
    pub target: Json,
    pub enabled_features: Json,
    pub document: Json,
    pub node_count: i32,
    pub max_depth: i32,
    pub cycle_count: i32,
    /// Ordered edge index. Ordinals are assigned from this vector, not trusted
    /// from callers.
    pub edges: Vec<DependencyGraphEdgeInput>,
}

#[cfg(feature = "read-write")]
#[derive(Clone, Debug, PartialEq)]
pub struct DependencyGraphEdgeInput {
    pub from_registry_id: String,
    pub from_org_slug: String,
    pub from_package_name: String,
    pub from_version: Option<String>,
    pub from_package_id: Option<Uuid>,
    pub from_package_version_id: Option<Uuid>,
    pub to_registry_id: String,
    pub to_org_slug: String,
    pub to_package_name: String,
    pub to_version: Option<String>,
    pub to_package_id: Option<Uuid>,
    pub to_package_version_id: Option<Uuid>,
    pub requirement: Option<String>,
    pub dependency_kind: String,
    pub optional: bool,
    pub default_features: bool,
    pub features: Json,
    pub target: Option<String>,
    pub minimum_depth: i32,
}

#[cfg(feature = "read-write")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyGraphPersistReceipt {
    pub graph_artifact_id: Uuid,
    pub graph_digest: String,
    /// True only when this call inserted the immutable document and edge index.
    pub inserted: bool,
}

/// Atomically persist one immutable graph document and its normalized edges.
///
/// Replaying byte-for-byte-equivalent persistence facts returns the original
/// artifact id. Reusing a semantic digest (or a declared root version) with
/// different facts fails closed rather than mutating graph history.
#[cfg(feature = "read-write")]
pub async fn persist_dependency_graph(
    context: &WriteContext,
    input: DependencyGraphArtifactInput,
) -> Result<DependencyGraphPersistReceipt, OrmError> {
    validate_artifact_input(&input)?;
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;

    lock_graph_key(&transaction, &format!("digest:{}", input.graph_digest)).await?;
    lock_graph_key(
        &transaction,
        &format!("root:{}", input.root_package_version_id),
    )
    .await?;

    let root_exists = package_version::Entity::find_by_id(input.root_package_version_id)
        .one(&transaction)
        .await
        .map_err(OrmError::from_db_err)?
        .is_some();
    if !root_exists {
        return Err(OrmError::not_found("root package version"));
    }

    if let Some(existing) = dependency_graph_artifact::Entity::find()
        .filter(dependency_graph_artifact::Column::GraphDigest.eq(&input.graph_digest))
        .one(&transaction)
        .await
        .map_err(OrmError::from_db_err)?
    {
        require_exact_replay(&transaction, &existing, &input).await?;
        transaction.commit().await.map_err(OrmError::from_db_err)?;
        return Ok(DependencyGraphPersistReceipt {
            graph_artifact_id: existing.id,
            graph_digest: existing.graph_digest,
            inserted: false,
        });
    }

    if input.graph_kind == "declared" {
        let existing_declared = dependency_graph_artifact::Entity::find()
            .filter(
                dependency_graph_artifact::Column::RootPackageVersionId
                    .eq(input.root_package_version_id),
            )
            .filter(dependency_graph_artifact::Column::GraphKind.eq("declared"))
            .one(&transaction)
            .await
            .map_err(OrmError::from_db_err)?;
        if existing_declared.is_some() {
            return Err(OrmError::policy(
                "root package version already has a different declared dependency graph",
            ));
        }
    }

    let created_at = chrono::Utc::now().fixed_offset();
    let artifact = dependency_graph_artifact::ActiveModel {
        id: Set(Uuid::new_v4()),
        root_package_version_id: Set(input.root_package_version_id),
        graph_kind: Set(input.graph_kind.clone()),
        schema_version: Set(input.schema_version.clone()),
        graph_digest: Set(input.graph_digest.clone()),
        resolver_name: Set(input.resolver_name.clone()),
        resolver_version: Set(input.resolver_version.clone()),
        resolution_input_digest: Set(input.resolution_input_digest.clone()),
        registry_checkpoint: Set(input.registry_checkpoint.clone()),
        target: Set(input.target.clone()),
        enabled_features: Set(input.enabled_features.clone()),
        document: Set(input.document.clone()),
        node_count: Set(input.node_count),
        edge_count: Set(i32::try_from(input.edges.len()).expect("validated edge count fits i32")),
        max_depth: Set(input.max_depth),
        cycle_count: Set(input.cycle_count),
        created_at: Set(created_at),
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;

    if !input.edges.is_empty() {
        let edges = input
            .edges
            .iter()
            .enumerate()
            .map(|(ordinal, edge)| dependency_graph_edge::ActiveModel {
                id: Set(Uuid::new_v4()),
                graph_artifact_id: Set(artifact.id),
                ordinal: Set(i32::try_from(ordinal).expect("validated edge ordinal fits i32")),
                from_registry_id: Set(edge.from_registry_id.clone()),
                from_org_slug: Set(edge.from_org_slug.clone()),
                from_package_name: Set(edge.from_package_name.clone()),
                from_version: Set(edge.from_version.clone()),
                from_package_id: Set(edge.from_package_id),
                from_package_version_id: Set(edge.from_package_version_id),
                to_registry_id: Set(edge.to_registry_id.clone()),
                to_org_slug: Set(edge.to_org_slug.clone()),
                to_package_name: Set(edge.to_package_name.clone()),
                to_version: Set(edge.to_version.clone()),
                to_package_id: Set(edge.to_package_id),
                to_package_version_id: Set(edge.to_package_version_id),
                requirement: Set(edge.requirement.clone()),
                dependency_kind: Set(edge.dependency_kind.clone()),
                optional: Set(edge.optional),
                default_features: Set(edge.default_features),
                features: Set(edge.features.clone()),
                target: Set(edge.target.clone()),
                minimum_depth: Set(edge.minimum_depth),
                created_at: Set(created_at),
            })
            .collect::<Vec<_>>();
        dependency_graph_edge::Entity::insert_many(edges)
            .exec(&transaction)
            .await
            .map_err(OrmError::from_db_err)?;
    }

    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(DependencyGraphPersistReceipt {
        graph_artifact_id: artifact.id,
        graph_digest: artifact.graph_digest,
        inserted: true,
    })
}

#[cfg(feature = "read-write")]
async fn lock_graph_key<C>(connection: &C, key: &str) -> Result<(), OrmError>
where
    C: ConnectionTrait,
{
    connection
        .execute(Statement::from_sql_and_values(
            connection.get_database_backend(),
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            [Value::String(Some(Box::new(format!(
                "zed-dependency-graph:{key}"
            ))))],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(())
}

#[cfg(feature = "read-write")]
async fn require_exact_replay<C>(
    connection: &C,
    artifact: &dependency_graph_artifact::Model,
    input: &DependencyGraphArtifactInput,
) -> Result<(), OrmError>
where
    C: ConnectionTrait,
{
    if !artifact_matches(artifact, input) {
        return Err(OrmError::policy(
            "dependency graph digest already exists with different immutable artifact facts",
        ));
    }
    let edges = load_edges(connection, artifact.id).await?;
    if edges.len() != input.edges.len()
        || edges
            .iter()
            .zip(&input.edges)
            .enumerate()
            .any(|(ordinal, (stored, candidate))| {
                stored.ordinal != i32::try_from(ordinal).expect("validated edge ordinal fits i32")
                    || !edge_matches(stored, candidate)
            })
    {
        return Err(OrmError::policy(
            "dependency graph digest already exists with a different normalized edge index",
        ));
    }
    Ok(())
}

#[cfg(feature = "read-write")]
fn artifact_matches(
    artifact: &dependency_graph_artifact::Model,
    input: &DependencyGraphArtifactInput,
) -> bool {
    artifact.root_package_version_id == input.root_package_version_id
        && artifact.graph_kind == input.graph_kind
        && artifact.schema_version == input.schema_version
        && artifact.graph_digest == input.graph_digest
        && artifact.resolver_name == input.resolver_name
        && artifact.resolver_version == input.resolver_version
        && artifact.resolution_input_digest == input.resolution_input_digest
        && artifact.registry_checkpoint == input.registry_checkpoint
        && artifact.target == input.target
        && artifact.enabled_features == input.enabled_features
        && artifact.document == input.document
        && artifact.node_count == input.node_count
        && artifact.edge_count == i32::try_from(input.edges.len()).unwrap_or(i32::MAX)
        && artifact.max_depth == input.max_depth
        && artifact.cycle_count == input.cycle_count
}

#[cfg(feature = "read-write")]
fn edge_matches(edge: &dependency_graph_edge::Model, input: &DependencyGraphEdgeInput) -> bool {
    edge.from_registry_id == input.from_registry_id
        && edge.from_org_slug == input.from_org_slug
        && edge.from_package_name == input.from_package_name
        && edge.from_version == input.from_version
        && edge.from_package_id == input.from_package_id
        && edge.from_package_version_id == input.from_package_version_id
        && edge.to_registry_id == input.to_registry_id
        && edge.to_org_slug == input.to_org_slug
        && edge.to_package_name == input.to_package_name
        && edge.to_version == input.to_version
        && edge.to_package_id == input.to_package_id
        && edge.to_package_version_id == input.to_package_version_id
        && edge.requirement == input.requirement
        && edge.dependency_kind == input.dependency_kind
        && edge.optional == input.optional
        && edge.default_features == input.default_features
        && edge.features == input.features
        && edge.target == input.target
        && edge.minimum_depth == input.minimum_depth
}

#[cfg(feature = "read-write")]
fn validate_artifact_input(input: &DependencyGraphArtifactInput) -> Result<(), OrmError> {
    validate_graph_kind(&input.graph_kind)?;
    validate_required_length("graph schema version", &input.schema_version, 96)?;
    validate_prefixed_sha256("graph digest", &input.graph_digest)?;
    optional_text("resolver name", input.resolver_name.as_deref(), 120)?;
    optional_text("resolver version", input.resolver_version.as_deref(), 120)?;
    if let Some(digest) = input.resolution_input_digest.as_deref() {
        validate_prefixed_sha256("resolution input digest", digest)?;
    }
    optional_text(
        "registry checkpoint",
        input.registry_checkpoint.as_deref(),
        1_024,
    )?;
    match input.graph_kind.as_str() {
        "declared"
            if input.resolver_name.is_some()
                || input.resolver_version.is_some()
                || input.resolution_input_digest.is_some() =>
        {
            return Err(OrmError::policy(
                "declared graphs cannot carry resolver or resolution-input evidence",
            ));
        }
        "resolved"
            if input.resolver_name.as_deref().is_none_or(str::is_empty)
                || input.resolver_version.as_deref().is_none_or(str::is_empty)
                || input.resolution_input_digest.is_none() =>
        {
            return Err(OrmError::policy(
                "resolved graphs require resolver name, version, and resolution-input digest",
            ));
        }
        _ => {}
    }
    if !input.target.is_object() {
        return Err(OrmError::policy("graph target must be a JSON object"));
    }
    if !input.enabled_features.is_array() {
        return Err(OrmError::policy(
            "graph enabled features must be a JSON array",
        ));
    }
    if !input.document.is_object() {
        return Err(OrmError::policy("graph document must be a JSON object"));
    }
    if input.node_count < 1
        || input.max_depth < 0
        || input.cycle_count < 0
        || input.edges.len() > i32::MAX as usize
    {
        return Err(OrmError::policy(
            "graph counts and depth must fit the nonnegative registry bounds",
        ));
    }
    for edge in &input.edges {
        validate_edge_input(edge)?;
        if edge.minimum_depth > input.max_depth {
            return Err(OrmError::policy(
                "edge minimum depth cannot exceed the graph maximum depth",
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "read-write")]
fn validate_edge_input(input: &DependencyGraphEdgeInput) -> Result<(), OrmError> {
    validate_required_length("source registry id", &input.from_registry_id, 512)?;
    validate_org_slug("source organization slug", &input.from_org_slug)?;
    validate_package_name("source package name", &input.from_package_name)?;
    validate_optional_nonempty("source version", input.from_version.as_deref(), 128)?;
    validate_required_length("target registry id", &input.to_registry_id, 512)?;
    validate_org_slug("target organization slug", &input.to_org_slug)?;
    validate_package_name("target package name", &input.to_package_name)?;
    validate_optional_nonempty("target version", input.to_version.as_deref(), 128)?;
    validate_optional_nonempty(
        "dependency requirement",
        input.requirement.as_deref(),
        1_024,
    )?;
    one_of("dependency kind", &input.dependency_kind, DEPENDENCY_KINDS)?;
    if !input.features.is_array() {
        return Err(OrmError::policy("dependency features must be a JSON array"));
    }
    optional_text("dependency target", input.target.as_deref(), 512)?;
    if input.minimum_depth < 1 {
        return Err(OrmError::policy(
            "dependency minimum depth must be at least one",
        ));
    }
    if input.from_package_version_id.is_some()
        && (input.from_package_id.is_none() || input.from_version.is_none())
    {
        return Err(OrmError::policy(
            "source package-version ids require source package ids and version spellings",
        ));
    }
    if input.to_package_version_id.is_some()
        && (input.to_package_id.is_none() || input.to_version.is_none())
    {
        return Err(OrmError::policy(
            "target package-version ids require target package ids and version spellings",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_digests_are_prefixed_lowercase_sha256() {
        assert!(validate_prefixed_sha256("digest", &format!("sha256:{}", "a".repeat(64))).is_ok());
        assert!(validate_prefixed_sha256("digest", &"a".repeat(64)).is_err());
        assert!(validate_prefixed_sha256("digest", &format!("sha256:{}", "A".repeat(64))).is_err());
    }

    #[test]
    fn graph_coordinates_match_shared_schema_names() {
        let coordinate = DependencyGraphCoordinate {
            registry_id: "https://api.zpkg.net/v1/registry".to_owned(),
            org_slug: "zed-pkg".to_owned(),
            package_name: "zed-lib-core".to_owned(),
            version: Some("1.0.0-rc.1".to_owned()),
        };
        assert!(validate_coordinate(&coordinate).is_ok());
    }

    #[cfg(feature = "read-write")]
    fn edge() -> DependencyGraphEdgeInput {
        DependencyGraphEdgeInput {
            from_registry_id: "zpkg".to_owned(),
            from_org_slug: "zed-pkg".to_owned(),
            from_package_name: "zed-api".to_owned(),
            from_version: Some("1.0.0".to_owned()),
            from_package_id: None,
            from_package_version_id: None,
            to_registry_id: "zpkg".to_owned(),
            to_org_slug: "zed-pkg".to_owned(),
            to_package_name: "zed-core".to_owned(),
            to_version: Some("2.0.0".to_owned()),
            to_package_id: None,
            to_package_version_id: None,
            requirement: Some("^2".to_owned()),
            dependency_kind: "runtime".to_owned(),
            optional: false,
            default_features: true,
            features: serde_json::json!([]),
            target: None,
            minimum_depth: 1,
        }
    }

    #[cfg(feature = "read-write")]
    fn artifact(graph_kind: &str) -> DependencyGraphArtifactInput {
        DependencyGraphArtifactInput {
            root_package_version_id: Uuid::nil(),
            graph_kind: graph_kind.to_owned(),
            schema_version: "zed-pkg/dependency-graph/v1".to_owned(),
            graph_digest: format!("sha256:{}", "a".repeat(64)),
            resolver_name: None,
            resolver_version: None,
            resolution_input_digest: None,
            registry_checkpoint: None,
            target: serde_json::json!({}),
            enabled_features: serde_json::json!([]),
            document: serde_json::json!({"nodes": [], "edges": []}),
            node_count: 2,
            max_depth: 1,
            cycle_count: 0,
            edges: vec![edge()],
        }
    }

    #[cfg(feature = "read-write")]
    #[test]
    fn declared_and_resolved_evidence_shapes_are_disjoint() {
        assert!(validate_artifact_input(&artifact("declared")).is_ok());
        let mut resolved = artifact("resolved");
        assert!(validate_artifact_input(&resolved).is_err());
        resolved.resolver_name = Some("zed-resolver".to_owned());
        resolved.resolver_version = Some("1.0.0".to_owned());
        resolved.resolution_input_digest = Some(format!("sha256:{}", "b".repeat(64)));
        assert!(validate_artifact_input(&resolved).is_ok());
    }

    #[cfg(feature = "read-write")]
    #[test]
    fn edge_index_requires_array_features_and_bounded_depth() {
        let mut input = artifact("declared");
        input.edges[0].features = serde_json::json!({});
        assert!(validate_artifact_input(&input).is_err());
        input.edges[0].features = serde_json::json!([]);
        input.edges[0].minimum_depth = 2;
        assert!(validate_artifact_input(&input).is_err());
    }
}
