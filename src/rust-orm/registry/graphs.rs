//! Immutable dependency-graph documents and their normalized edge index.
//!
//! The JSON document is the lossless serialization authority. Edge rows are
//! derived query accelerators for reverse-impact and neighborhood reads; the
//! write path commits both representations atomically and rejects divergent
//! replays of an existing semantic digest.

#[cfg(feature = "read-write")]
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use sea_orm::{
    prelude::Uuid, ColumnTrait, Condition, ConnectionTrait, EntityTrait, JoinType, QueryFilter,
    QueryOrder, QuerySelect, RelationTrait, Select,
};

#[cfg(feature = "read-write")]
use sea_orm::{
    prelude::Json, ActiveModelTrait, ActiveValue::Set, Statement, TransactionTrait, Value,
};

#[cfg(feature = "read-write")]
use zed_interfaces::dependency_graph::{
    DependencyGraphData, DependencyGraphDocument, DependencyKind, PackageVersionIdentity,
    ResolvedDependencyEdge, ResolvedDependencyNode, DEPENDENCY_GRAPH_DEFAULT_MAX_EDGES,
    DEPENDENCY_GRAPH_DEFAULT_MAX_ENCODED_BYTES, DEPENDENCY_GRAPH_DEFAULT_MAX_NODES,
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
        .order_by_desc(dependency_graph_artifact::Column::Id)
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

/// Visible graph edges which originate at a registry package coordinate.
///
/// This is the forward-neighborhood primitive used to walk dependency paths.
/// As with reverse impact, visibility is determined by each graph's root
/// package so an edge from a private graph is never exposed independently.
pub async fn outgoing_dependency_edges(
    context: &ReadContext,
    coordinate: &DependencyGraphCoordinate,
    visible_org_ids: &[Uuid],
    limit: u64,
) -> Result<Vec<dependency_graph_edge::Model>, OrmError> {
    validate_coordinate(coordinate)?;
    let mut query = visible_edges(visible_org_ids)
        .filter(dependency_graph_edge::Column::FromRegistryId.eq(coordinate.registry_id.as_str()))
        .filter(dependency_graph_edge::Column::FromOrgSlug.eq(coordinate.org_slug.as_str()))
        .filter(
            dependency_graph_edge::Column::FromPackageName.eq(coordinate.package_name.as_str()),
        );
    if let Some(version) = coordinate.version.as_deref() {
        query = query.filter(dependency_graph_edge::Column::FromVersion.eq(version));
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
        .filter(dependency_graph_artifact::Column::SealedAt.is_not_null())
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
        .filter(dependency_graph_artifact::Column::SealedAt.is_not_null())
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
    let validated = validate_artifact_input(&input)?;
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

    let root_version = package_version::Entity::find_by_id(input.root_package_version_id)
        .one(&transaction)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| OrmError::not_found("root package version"))?;
    validate_root_identity(&transaction, &root_version, &validated.document).await?;
    validate_edge_links(&transaction, &input.edges).await?;

    if let Some(existing) = dependency_graph_artifact::Entity::find()
        .filter(dependency_graph_artifact::Column::GraphDigest.eq(&input.graph_digest))
        .one(&transaction)
        .await
        .map_err(OrmError::from_db_err)?
    {
        if existing.sealed_at.is_none() {
            return Err(OrmError::policy(
                "dependency graph digest exists in an unsealed state and requires repair",
            ));
        }
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
        sealed_at: Set(None),
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

    let mut sealed: dependency_graph_artifact::ActiveModel = artifact.into();
    sealed.sealed_at = Set(Some(chrono::Utc::now().fixed_offset()));
    let artifact = sealed
        .update(&transaction)
        .await
        .map_err(OrmError::from_db_err)?;

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
async fn validate_root_identity<C>(
    connection: &C,
    root_version: &package_version::Model,
    document: &DependencyGraphDocument,
) -> Result<(), OrmError>
where
    C: ConnectionTrait,
{
    let root_package = package::Entity::find_by_id(root_version.package_id)
        .one(connection)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| OrmError::not_found("root package"))?;
    let root_org = org::Entity::find_by_id(root_package.org_id)
        .one(connection)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| OrmError::not_found("root package organization"))?;
    if root_package.is_soft_deleted || root_org.is_soft_deleted {
        return Err(OrmError::not_found("active root package version"));
    }

    let matches_root = |identity: &PackageVersionIdentity| {
        identity.org == root_org.slug
            && identity.name == root_package.name
            && identity.version == root_version.version
    };
    let matches = match &document.graph {
        DependencyGraphData::Declared { package, .. } => matches_root(package),
        DependencyGraphData::Resolved { roots, .. } => roots.first().is_some_and(matches_root),
    };
    if !matches {
        return Err(OrmError::policy(
            "root package-version id does not match an exact root coordinate in the canonical document",
        ));
    }
    Ok(())
}

#[cfg(feature = "read-write")]
async fn validate_edge_links<C>(
    connection: &C,
    edges: &[DependencyGraphEdgeInput],
) -> Result<(), OrmError>
where
    C: ConnectionTrait,
{
    const LOOKUP_BATCH: usize = 10_000;

    let package_ids = edges
        .iter()
        .flat_map(|edge| [edge.from_package_id, edge.to_package_id])
        .flatten()
        .collect::<BTreeSet<_>>();
    let version_ids = edges
        .iter()
        .flat_map(|edge| [edge.from_package_version_id, edge.to_package_version_id])
        .flatten()
        .collect::<BTreeSet<_>>();

    let mut packages = BTreeMap::new();
    let package_ids = package_ids.into_iter().collect::<Vec<_>>();
    for ids in package_ids.chunks(LOOKUP_BATCH) {
        for model in package::Entity::find()
            .filter(package::Column::Id.is_in(ids.to_vec()))
            .all(connection)
            .await
            .map_err(OrmError::from_db_err)?
        {
            packages.insert(model.id, model);
        }
    }

    let org_ids = packages
        .values()
        .map(|model| model.org_id)
        .collect::<BTreeSet<_>>();
    let mut orgs = BTreeMap::new();
    let org_ids = org_ids.into_iter().collect::<Vec<_>>();
    for ids in org_ids.chunks(LOOKUP_BATCH) {
        for model in org::Entity::find()
            .filter(org::Column::Id.is_in(ids.to_vec()))
            .all(connection)
            .await
            .map_err(OrmError::from_db_err)?
        {
            orgs.insert(model.id, model);
        }
    }

    let mut versions = BTreeMap::new();
    let version_ids = version_ids.into_iter().collect::<Vec<_>>();
    for ids in version_ids.chunks(LOOKUP_BATCH) {
        for model in package_version::Entity::find()
            .filter(package_version::Column::Id.is_in(ids.to_vec()))
            .all(connection)
            .await
            .map_err(OrmError::from_db_err)?
        {
            versions.insert(model.id, model);
        }
    }

    for edge in edges {
        validate_endpoint_link(
            "source",
            &edge.from_org_slug,
            &edge.from_package_name,
            edge.from_version.as_deref(),
            edge.from_package_id,
            edge.from_package_version_id,
            &packages,
            &orgs,
            &versions,
        )?;
        validate_endpoint_link(
            "target",
            &edge.to_org_slug,
            &edge.to_package_name,
            edge.to_version.as_deref(),
            edge.to_package_id,
            edge.to_package_version_id,
            &packages,
            &orgs,
            &versions,
        )?;
    }
    Ok(())
}

#[cfg(feature = "read-write")]
#[allow(clippy::too_many_arguments)]
fn validate_endpoint_link(
    label: &str,
    org_slug: &str,
    package_name: &str,
    version: Option<&str>,
    package_id: Option<Uuid>,
    package_version_id: Option<Uuid>,
    packages: &BTreeMap<Uuid, package::Model>,
    orgs: &BTreeMap<Uuid, org::Model>,
    versions: &BTreeMap<Uuid, package_version::Model>,
) -> Result<(), OrmError> {
    let Some(package_id) = package_id else {
        return Ok(());
    };
    let linked_package = packages
        .get(&package_id)
        .ok_or_else(|| OrmError::policy(format!("graph {label} package id does not exist")))?;
    let linked_org = orgs.get(&linked_package.org_id).ok_or_else(|| {
        OrmError::policy(format!("graph {label} package organization does not exist"))
    })?;
    if linked_package.is_soft_deleted
        || linked_org.is_soft_deleted
        || linked_package.name != package_name
        || linked_org.slug != org_slug
    {
        return Err(OrmError::policy(format!(
            "graph {label} package id does not match its canonical organization/name coordinate"
        )));
    }

    if let Some(package_version_id) = package_version_id {
        let linked_version = versions.get(&package_version_id).ok_or_else(|| {
            OrmError::policy(format!("graph {label} package-version id does not exist"))
        })?;
        if linked_version.package_id != package_id
            || version != Some(linked_version.version.as_str())
        {
            return Err(OrmError::policy(format!(
                "graph {label} package-version id does not match its package id and exact published spelling"
            )));
        }
    }
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
struct ValidatedGraph {
    document: DependencyGraphDocument,
}

#[cfg(feature = "read-write")]
struct GraphProjection {
    node_count: i32,
    max_depth: i32,
    cycle_count: i32,
    edge_depths: Vec<i32>,
}

#[cfg(feature = "read-write")]
fn validate_artifact_input(
    input: &DependencyGraphArtifactInput,
) -> Result<ValidatedGraph, OrmError> {
    validate_graph_kind(&input.graph_kind)?;
    validate_required_length("graph schema version", &input.schema_version, 96)?;
    validate_prefixed_sha256("graph digest", &input.graph_digest)?;
    optional_text("resolver name", input.resolver_name.as_deref(), 120)?;
    optional_text("resolver version", input.resolver_version.as_deref(), 120)?;
    if let Some(digest) = input.resolution_input_digest.as_deref() {
        validate_prefixed_sha256("resolution input digest", digest)?;
    }
    validate_optional_nonempty(
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

    let document: DependencyGraphDocument = serde_json::from_value(input.document.clone())
        .map_err(|error| {
            OrmError::policy(format!(
                "graph document does not satisfy the shared typed contract: {error}"
            ))
        })?;
    document.verify_digest().map_err(|error| {
        OrmError::policy(format!(
            "graph document failed canonical semantic-digest verification: {error}"
        ))
    })?;
    let canonical_bytes = document.canonical_document_bytes().map_err(|error| {
        OrmError::policy(format!(
            "graph document cannot be serialized canonically: {error}"
        ))
    })?;
    if canonical_bytes.len() as u64 > DEPENDENCY_GRAPH_DEFAULT_MAX_ENCODED_BYTES {
        return Err(OrmError::policy(format!(
            "canonical graph document exceeds the {}-byte persistence limit",
            DEPENDENCY_GRAPH_DEFAULT_MAX_ENCODED_BYTES
        )));
    }
    let canonical_document: Json = serde_json::from_slice(&canonical_bytes)
        .map_err(|error| OrmError::policy(format!("canonical graph JSON is invalid: {error}")))?;
    if canonical_document != input.document {
        return Err(OrmError::policy(
            "graph document must be the normalized typed document without unknown members or explicit null optionals",
        ));
    }
    if document.graph_digest.as_deref() != Some(input.graph_digest.as_str()) {
        return Err(OrmError::policy(
            "graph digest column must equal the verified digest embedded in the document",
        ));
    }
    if document.schema != input.schema_version {
        return Err(OrmError::policy(
            "graph schema-version column must equal the schema embedded in the document",
        ));
    }
    let document_kind = match &document.graph {
        DependencyGraphData::Declared { .. } => "declared",
        DependencyGraphData::Resolved { .. } => "resolved",
    };
    if input.graph_kind != document_kind {
        return Err(OrmError::policy(
            "graph-kind column must equal the declared or resolved view embedded in the document",
        ));
    }
    validate_persisted_root_shape(&document)?;
    match &document.graph {
        DependencyGraphData::Declared { .. }
            if input.registry_checkpoint.is_some()
                || input.target != serde_json::json!({})
                || input.enabled_features != serde_json::json!([]) =>
        {
            return Err(OrmError::policy(
                "declared graphs cannot carry artifact-level checkpoint, target, or enabled-feature resolution inputs",
            ));
        }
        DependencyGraphData::Resolved { provenance, .. }
            if input.resolver_version.as_deref() != Some(provenance.resolver_version.as_str()) =>
        {
            return Err(OrmError::policy(
                "resolved graph resolver version must equal the canonical resolution provenance",
            ));
        }
        DependencyGraphData::Resolved { provenance, .. }
            if input.enabled_features != serde_json::json!(provenance.enabled_features) =>
        {
            return Err(OrmError::policy(
                "resolved graph enabled features must equal the canonical resolution provenance",
            ));
        }
        _ => {}
    }

    let projection = graph_projection(&document)?;
    if input.node_count != projection.node_count {
        return Err(OrmError::policy(format!(
            "graph node count must equal the {} nodes derived from the canonical document",
            projection.node_count
        )));
    }
    if input.max_depth != projection.max_depth {
        return Err(OrmError::policy(format!(
            "graph maximum depth must equal the {}-level shortest-path projection derived from the canonical document",
            projection.max_depth
        )));
    }
    if input.cycle_count != projection.cycle_count {
        return Err(OrmError::policy(format!(
            "graph cycle count must equal the {} cyclic components derived from the canonical document",
            projection.cycle_count
        )));
    }
    if input.edges.len() != projection.edge_depths.len() {
        return Err(OrmError::policy(
            "normalized edge count must equal the canonical document edge count",
        ));
    }
    for (ordinal, (edge, minimum_depth)) in
        input.edges.iter().zip(&projection.edge_depths).enumerate()
    {
        validate_edge_input(edge)?;
        if edge.minimum_depth != *minimum_depth {
            return Err(OrmError::policy(format!(
                "normalized edge {ordinal} has a minimum depth that diverges from the canonical document"
            )));
        }
        if !edge_matches_document(&document, ordinal, edge) {
            return Err(OrmError::policy(format!(
                "normalized edge {ordinal} diverges from the canonical graph document"
            )));
        }
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
    if input.node_count as u32 > DEPENDENCY_GRAPH_DEFAULT_MAX_NODES
        || input.edges.len() as u32 > DEPENDENCY_GRAPH_DEFAULT_MAX_EDGES
    {
        return Err(OrmError::policy(format!(
            "graph exceeds the default persistence limits of {} nodes and {} edges",
            DEPENDENCY_GRAPH_DEFAULT_MAX_NODES, DEPENDENCY_GRAPH_DEFAULT_MAX_EDGES
        )));
    }
    Ok(ValidatedGraph { document })
}

#[cfg(feature = "read-write")]
fn validate_persisted_root_shape(document: &DependencyGraphDocument) -> Result<(), OrmError> {
    if let DependencyGraphData::Resolved { roots, .. } = &document.graph {
        if roots.len() != 1 {
            return Err(OrmError::policy(
                "resolved graph persistence requires exactly one root because visibility and the root foreign key are singular",
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "read-write")]
fn graph_projection(document: &DependencyGraphDocument) -> Result<GraphProjection, OrmError> {
    match &document.graph {
        DependencyGraphData::Declared {
            package,
            dependencies,
        } => {
            let dependency_nodes = dependencies
                .iter()
                .map(|dependency| {
                    (
                        dependency.registry_id.as_str(),
                        dependency.org.as_str(),
                        dependency.name.as_str(),
                    )
                })
                .collect::<BTreeSet<_>>()
                .len();
            let node_count = 1usize
                .checked_add(dependency_nodes)
                .and_then(|count| i32::try_from(count).ok())
                .ok_or_else(|| OrmError::policy("declared graph node count exceeds i32"))?;
            let _ = package;
            Ok(GraphProjection {
                node_count,
                max_depth: i32::from(!dependencies.is_empty()),
                cycle_count: 0,
                edge_depths: vec![1; dependencies.len()],
            })
        }
        DependencyGraphData::Resolved {
            roots,
            nodes,
            edges,
            ..
        } => {
            let node_count = i32::try_from(nodes.len())
                .map_err(|_| OrmError::policy("resolved graph node count exceeds i32"))?;
            let analysis = analyze_resolved_graph(roots, nodes, edges)?;
            Ok(GraphProjection {
                node_count,
                max_depth: analysis.max_depth,
                cycle_count: analysis.cycle_count,
                edge_depths: analysis.edge_depths,
            })
        }
    }
}

#[cfg(feature = "read-write")]
struct ResolvedGraphAnalysis {
    max_depth: i32,
    cycle_count: i32,
    edge_depths: Vec<i32>,
}

#[cfg(feature = "read-write")]
fn analyze_resolved_graph(
    roots: &[PackageVersionIdentity],
    nodes: &[ResolvedDependencyNode],
    edges: &[ResolvedDependencyEdge],
) -> Result<ResolvedGraphAnalysis, OrmError> {
    let indices = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = vec![Vec::new(); nodes.len()];
    let mut incoming = vec![Vec::new(); nodes.len()];
    let mut self_loop = vec![false; nodes.len()];
    for edge in edges {
        let from = *indices
            .get(&edge.from)
            .ok_or_else(|| OrmError::policy("resolved graph edge source is not a node"))?;
        let to = *indices
            .get(&edge.to)
            .ok_or_else(|| OrmError::policy("resolved graph edge target is not a node"))?;
        outgoing[from].push(to);
        incoming[to].push(from);
        self_loop[from] |= from == to;
    }

    let mut depths = vec![None; nodes.len()];
    let mut queue = VecDeque::new();
    for root in roots {
        let root = *indices
            .get(root)
            .ok_or_else(|| OrmError::policy("resolved graph root is not a node"))?;
        if depths[root].is_none() {
            depths[root] = Some(0i32);
            queue.push_back(root);
        }
    }
    while let Some(node) = queue.pop_front() {
        let next_depth = depths[node]
            .expect("queued graph nodes have depths")
            .checked_add(1)
            .ok_or_else(|| OrmError::policy("resolved graph depth exceeds i32"))?;
        for &neighbor in &outgoing[node] {
            if depths[neighbor].is_none() {
                depths[neighbor] = Some(next_depth);
                queue.push_back(neighbor);
            }
        }
    }
    if depths.iter().any(Option::is_none) {
        return Err(OrmError::policy(
            "resolved graph contains a node that is unreachable from every root",
        ));
    }
    let edge_depths = edges
        .iter()
        .map(|edge| {
            let target = indices[&edge.to];
            depths[target]
                .expect("all graph nodes are reachable")
                .max(1)
        })
        .collect::<Vec<_>>();
    let max_depth = depths
        .into_iter()
        .flatten()
        .max()
        .unwrap_or_default()
        .max(i32::from(!edges.is_empty()));
    let cycle_count = count_cyclic_components(&outgoing, &incoming, &self_loop)?;
    Ok(ResolvedGraphAnalysis {
        max_depth,
        cycle_count,
        edge_depths,
    })
}

#[cfg(feature = "read-write")]
fn count_cyclic_components(
    outgoing: &[Vec<usize>],
    incoming: &[Vec<usize>],
    self_loop: &[bool],
) -> Result<i32, OrmError> {
    debug_assert_eq!(outgoing.len(), incoming.len());
    debug_assert_eq!(outgoing.len(), self_loop.len());

    let mut visited = vec![false; outgoing.len()];
    let mut finished = Vec::with_capacity(outgoing.len());
    for start in 0..outgoing.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![(start, 0usize)];
        while !stack.is_empty() {
            let frame = stack.len() - 1;
            let node = stack[frame].0;
            let next = stack[frame].1;
            if let Some(&neighbor) = outgoing[node].get(next) {
                stack[frame].1 += 1;
                if !visited[neighbor] {
                    visited[neighbor] = true;
                    stack.push((neighbor, 0));
                }
            } else {
                stack.pop();
                finished.push(node);
            }
        }
    }

    let mut assigned = vec![false; outgoing.len()];
    let mut cycle_count = 0i32;
    for start in finished.into_iter().rev() {
        if assigned[start] {
            continue;
        }
        assigned[start] = true;
        let mut stack = vec![start];
        let mut component_size = 0usize;
        let mut component_has_self_loop = false;
        while let Some(node) = stack.pop() {
            component_size += 1;
            component_has_self_loop |= self_loop[node];
            for &neighbor in &incoming[node] {
                if !assigned[neighbor] {
                    assigned[neighbor] = true;
                    stack.push(neighbor);
                }
            }
        }
        if component_size > 1 || component_has_self_loop {
            cycle_count = cycle_count
                .checked_add(1)
                .ok_or_else(|| OrmError::policy("graph cycle count exceeds i32"))?;
        }
    }
    Ok(cycle_count)
}

#[cfg(feature = "read-write")]
fn edge_matches_document(
    document: &DependencyGraphDocument,
    ordinal: usize,
    edge: &DependencyGraphEdgeInput,
) -> bool {
    match &document.graph {
        DependencyGraphData::Declared {
            package,
            dependencies,
        } => dependencies.get(ordinal).is_some_and(|dependency| {
            edge.from_registry_id == package.registry_id
                && edge.from_org_slug == package.org
                && edge.from_package_name == package.name
                && edge.from_version.as_deref() == Some(package.version.as_str())
                && edge.to_registry_id == dependency.registry_id
                && edge.to_org_slug == dependency.org
                && edge.to_package_name == dependency.name
                && edge.to_version.is_none()
                && edge.requirement.as_deref() == Some(dependency.requirement.as_str())
                && edge.dependency_kind == dependency_kind_name(dependency.kind)
                && edge.optional == dependency.optional
                && edge.default_features == dependency.default_features
                && edge.features == serde_json::json!(dependency.features)
                && edge.target == dependency.target
        }),
        DependencyGraphData::Resolved { edges, .. } => {
            edges.get(ordinal).is_some_and(|document_edge| {
                edge.from_registry_id == document_edge.from.registry_id
                    && edge.from_org_slug == document_edge.from.org
                    && edge.from_package_name == document_edge.from.name
                    && edge.from_version.as_deref() == Some(document_edge.from.version.as_str())
                    && edge.to_registry_id == document_edge.to.registry_id
                    && edge.to_org_slug == document_edge.to.org
                    && edge.to_package_name == document_edge.to.name
                    && edge.to_version.as_deref() == Some(document_edge.to.version.as_str())
                    && edge.requirement == document_edge.requirement
                    && edge.dependency_kind == dependency_kind_name(document_edge.kind)
                    && edge.optional == document_edge.optional
                    && edge.default_features
                    && edge.features == serde_json::json!(document_edge.features)
                    && edge.target == document_edge.target
            })
        }
    }
}

#[cfg(feature = "read-write")]
const fn dependency_kind_name(kind: DependencyKind) -> &'static str {
    match kind {
        DependencyKind::Runtime => "runtime",
        DependencyKind::Build => "build",
        DependencyKind::Development => "development",
        DependencyKind::Peer => "peer",
        DependencyKind::Tooling => "tooling",
    }
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

    #[test]
    fn graph_coordinate_versions_are_exact_nonempty_spellings() {
        let mut coordinate = DependencyGraphCoordinate {
            registry_id: "https://api.zpkg.net/v1/registry".to_owned(),
            org_slug: "zed-pkg".to_owned(),
            package_name: "zed-lib-core".to_owned(),
            version: Some("".to_owned()),
        };
        assert!(validate_coordinate(&coordinate).is_err());
        coordinate.version = Some("1.0.0+build.7".to_owned());
        assert!(validate_coordinate(&coordinate).is_ok());
    }

    #[cfg(feature = "read-write")]
    fn artifact(fixture: &str) -> DependencyGraphArtifactInput {
        let document = zed_interfaces::dependency_graph::golden_fixture_documents()
            .into_iter()
            .find_map(|(name, document)| (name == fixture).then_some(document))
            .expect("named dependency-graph fixture exists");
        let projection = graph_projection(&document).expect("golden fixture projects");
        let (graph_kind, resolver_name, resolver_version, resolution_input_digest, features, edges) =
            match &document.graph {
                DependencyGraphData::Declared {
                    package,
                    dependencies,
                } => (
                    "declared",
                    None,
                    None,
                    None,
                    serde_json::json!([]),
                    dependencies
                        .iter()
                        .enumerate()
                        .map(|(ordinal, dependency)| DependencyGraphEdgeInput {
                            from_registry_id: package.registry_id.clone(),
                            from_org_slug: package.org.clone(),
                            from_package_name: package.name.clone(),
                            from_version: Some(package.version.clone()),
                            from_package_id: None,
                            from_package_version_id: None,
                            to_registry_id: dependency.registry_id.clone(),
                            to_org_slug: dependency.org.clone(),
                            to_package_name: dependency.name.clone(),
                            to_version: None,
                            to_package_id: None,
                            to_package_version_id: None,
                            requirement: Some(dependency.requirement.clone()),
                            dependency_kind: dependency_kind_name(dependency.kind).to_owned(),
                            optional: dependency.optional,
                            default_features: dependency.default_features,
                            features: serde_json::json!(dependency.features),
                            target: dependency.target.clone(),
                            minimum_depth: projection.edge_depths[ordinal],
                        })
                        .collect(),
                ),
                DependencyGraphData::Resolved {
                    edges, provenance, ..
                } => (
                    "resolved",
                    Some("zed-resolver".to_owned()),
                    Some(provenance.resolver_version.clone()),
                    Some(format!("sha256:{}", "b".repeat(64))),
                    serde_json::json!(provenance.enabled_features),
                    edges
                        .iter()
                        .enumerate()
                        .map(|(ordinal, edge)| DependencyGraphEdgeInput {
                            from_registry_id: edge.from.registry_id.clone(),
                            from_org_slug: edge.from.org.clone(),
                            from_package_name: edge.from.name.clone(),
                            from_version: Some(edge.from.version.clone()),
                            from_package_id: None,
                            from_package_version_id: None,
                            to_registry_id: edge.to.registry_id.clone(),
                            to_org_slug: edge.to.org.clone(),
                            to_package_name: edge.to.name.clone(),
                            to_version: Some(edge.to.version.clone()),
                            to_package_id: None,
                            to_package_version_id: None,
                            requirement: edge.requirement.clone(),
                            dependency_kind: dependency_kind_name(edge.kind).to_owned(),
                            optional: edge.optional,
                            default_features: true,
                            features: serde_json::json!(edge.features),
                            target: edge.target.clone(),
                            minimum_depth: projection.edge_depths[ordinal],
                        })
                        .collect(),
                ),
            };
        DependencyGraphArtifactInput {
            root_package_version_id: Uuid::nil(),
            graph_kind: graph_kind.to_owned(),
            schema_version: document.schema.clone(),
            graph_digest: document
                .graph_digest
                .clone()
                .expect("golden fixture is finalized"),
            resolver_name,
            resolver_version,
            resolution_input_digest,
            registry_checkpoint: None,
            target: serde_json::json!({}),
            enabled_features: features,
            document: serde_json::to_value(document).expect("golden fixture serializes"),
            node_count: projection.node_count,
            max_depth: projection.max_depth,
            cycle_count: projection.cycle_count,
            edges,
        }
    }

    #[cfg(feature = "read-write")]
    #[test]
    fn declared_and_resolved_evidence_shapes_are_disjoint() {
        assert!(validate_artifact_input(&artifact("declared")).is_ok());
        assert!(validate_artifact_input(&artifact("diamond")).is_ok());

        let mut declared_with_resolution = artifact("declared");
        declared_with_resolution.resolver_name = Some("zed-resolver".to_owned());
        assert!(validate_artifact_input(&declared_with_resolution).is_err());

        let mut resolved_without_evidence = artifact("diamond");
        resolved_without_evidence.resolution_input_digest = None;
        assert!(validate_artifact_input(&resolved_without_evidence).is_err());

        let mut resolved_with_wrong_version = artifact("diamond");
        resolved_with_wrong_version.resolver_version = Some("different-resolver/9".to_owned());
        assert!(validate_artifact_input(&resolved_with_wrong_version).is_err());
    }

    #[cfg(feature = "read-write")]
    #[test]
    fn persisted_resolved_graphs_have_one_visibility_root() {
        let mut document: DependencyGraphDocument =
            serde_json::from_value(artifact("diamond").document).expect("golden graph parses");
        let DependencyGraphData::Resolved { roots, .. } = &mut document.graph else {
            panic!("diamond is resolved");
        };
        roots.push(roots[0].clone());
        assert!(validate_persisted_root_shape(&document).is_err());
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

    #[cfg(feature = "read-write")]
    #[test]
    fn semantic_digest_and_normalized_edge_index_are_authoritative() {
        let mut input = artifact("declared");
        input.graph_digest = format!("sha256:{}", "0".repeat(64));
        assert!(validate_artifact_input(&input).is_err());

        let mut input = artifact("declared");
        input.document["unknown"] = serde_json::json!(true);
        assert!(validate_artifact_input(&input).is_err());

        let mut input = artifact("declared");
        input.edges[0].to_package_name = "different-package".to_owned();
        assert!(validate_artifact_input(&input).is_err());
    }

    #[cfg(feature = "read-write")]
    #[test]
    fn graph_metrics_are_derived_from_reachable_canonical_nodes() {
        let mut input = artifact("diamond");
        input.node_count += 1;
        assert!(validate_artifact_input(&input).is_err());

        let mut input = artifact("diamond");
        input.max_depth += 1;
        assert!(validate_artifact_input(&input).is_err());

        let mut cycle = artifact("cycle");
        assert_eq!(cycle.cycle_count, 1);
        assert!(validate_artifact_input(&cycle).is_ok());
        cycle.cycle_count = 0;
        assert!(validate_artifact_input(&cycle).is_err());
    }
}
