//! Read-only, policy-aware named query functions.
//!
//! This module is the default consumer's entire view of the database. Business
//! reads added here must carry tenant/user scope and apply redaction. Generated
//! entities and raw query builders stay private to this crate. Prefer
//! `get_published_items_for_tenant(tenant_id)`-style named contracts over
//! anything that hands a caller a query builder.

use crate::{
    connection::{inspect_connection, InternalConnectionState},
    OrmError, ReadContext,
};

/// Safe, implementation-independent evidence about the active connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionState {
    schema: String,
    transaction_read_only: bool,
}

impl ConnectionState {
    pub(crate) fn from_internal(state: InternalConnectionState) -> Self {
        Self {
            schema: state.schema,
            transaction_read_only: state.transaction_read_only,
        }
    }

    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    #[must_use]
    pub fn transaction_read_only(&self) -> bool {
        self.transaction_read_only
    }
}

/// Return the verified policy state without exposing a SeaORM connection.
pub async fn connection_state(context: &ReadContext) -> Result<ConnectionState, OrmError> {
    inspect_connection(context.connection())
        .await
        .map(ConnectionState::from_internal)
}

/// Lightweight named readiness read for consumers and health checks.
pub async fn ping(context: &ReadContext) -> Result<(), OrmError> {
    let state = connection_state(context).await?;
    if state.transaction_read_only() {
        Ok(())
    } else {
        Err(OrmError::policy(
            "read context lost its read-only transaction policy",
        ))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Registry reads.
//
// Every function that can return a private row takes an explicit viewer scope.
// There is deliberately no "read this package" that decides visibility for the
// caller — the web tier's SELECT-only identity is a backstop, not an authorizer.
// ─────────────────────────────────────────────────────────────────────────────

use sea_orm::{
    ColumnTrait, Condition, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect,
    RelationTrait, Select,
};

use crate::entities::{
    org, org_member, package, package_license, package_version, project, project_member, user,
};
use crate::models::{OrgSummary, PackageSummary, ProjectSummary, UserSummary};
use crate::policy::VisibilityLimits;

/// Upper bound on any unpaginated listing, so one org cannot force a full scan.
pub const PAGE_LIMIT: u64 = 100;

/// The promotion window as the database currently defines it.
pub async fn visibility_limits(context: &ReadContext) -> Result<VisibilityLimits, OrmError> {
    VisibilityLimits::load(context.connection()).await
}

/// Resolve a Shared Auth principal to its registry user.
///
/// Keyed on `(realm, subject)` because a principal id is only unique within its
/// own auth instance.
pub async fn user_by_subject(
    context: &ReadContext,
    realm: &str,
    subject: uuid::Uuid,
) -> Result<Option<UserSummary>, OrmError> {
    let found = user::Entity::find()
        .filter(user::Column::AuthRealm.eq(realm))
        .filter(user::Column::SharedAuthSubject.eq(subject))
        .filter(user::Column::IsSoftDeleted.eq(false))
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(found.map(user_summary))
}

pub(crate) fn user_summary(model: user::Model) -> UserSummary {
    UserSummary {
        id: model.id,
        subject: model.shared_auth_subject,
        realm: model.auth_realm,
        email: model.email,
        display_name: model.display_name,
        avatar_url: model.avatar_url,
        settings: model.settings,
    }
}

/// Every org the user belongs to, with the role that membership grants.
///
/// This is the home page's primary query and the basis of the header's org
/// switcher.
pub async fn orgs_for_user(
    context: &ReadContext,
    user_id: uuid::Uuid,
) -> Result<Vec<OrgSummary>, OrmError> {
    let rows = org::Entity::find()
        .join(JoinType::InnerJoin, org::Relation::OrgMember.def())
        .filter(org_member::Column::UserId.eq(user_id))
        .filter(org::Column::IsSoftDeleted.eq(false))
        .select_also(org_member::Entity)
        .order_by_asc(org::Column::Slug)
        .limit(PAGE_LIMIT)
        .all(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;

    Ok(rows
        .into_iter()
        .map(|(org_row, membership)| OrgSummary {
            id: org_row.id,
            slug: org_row.slug,
            name: org_row.name,
            description: org_row.description,
            role: membership
                .map(|member| member.role)
                .unwrap_or_else(|| "reader".to_owned()),
        })
        .collect())
}

/// The viewer's role in an org, or `None` if they are not a member.
///
/// Callers use this to decide whether private rows may be shown at all.
pub async fn org_role_for_user(
    context: &ReadContext,
    org_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<Option<String>, OrmError> {
    let membership = org_member::Entity::find_by_id((org_id, user_id))
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(membership.map(|row| row.role))
}

pub async fn org_by_slug(
    context: &ReadContext,
    slug: &str,
) -> Result<Option<org::Model>, OrmError> {
    org::Entity::find()
        .filter(org::Column::Slug.eq(slug))
        .filter(org::Column::IsSoftDeleted.eq(false))
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)
}

/// Projects in an org. `include_private` must reflect a checked membership.
pub async fn projects_for_org(
    context: &ReadContext,
    org_id: uuid::Uuid,
    org_slug: &str,
    include_private: bool,
) -> Result<Vec<ProjectSummary>, OrmError> {
    let mut query = project::Entity::find()
        .filter(project::Column::OrgId.eq(org_id))
        .filter(project::Column::IsSoftDeleted.eq(false));
    if !include_private {
        query = query.filter(project::Column::Visibility.eq("public"));
    }

    let rows = query
        .order_by_asc(project::Column::Slug)
        .limit(PAGE_LIMIT)
        .all(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;

    Ok(rows
        .into_iter()
        .map(|row| ProjectSummary {
            id: row.id,
            org_id: row.org_id,
            org_slug: org_slug.to_owned(),
            slug: row.slug,
            name: row.name,
            description: row.description,
            role: String::new(),
        })
        .collect())
}

/// Resolve one active project at its canonical `{org}/{project}` coordinate.
///
/// This uses the schema's active-project unique key directly and intentionally
/// does not scan the paginated [`projects_for_org`] or [`projects_for_user`]
/// listings.
pub async fn project_by_org_and_slug(
    context: &ReadContext,
    org_id: uuid::Uuid,
    slug: &str,
) -> Result<Option<project::Model>, OrmError> {
    project_by_org_and_slug_query(org_id, slug)
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)
}

/// Resolve one active project by its immutable primary key.
///
/// This is used when a package stores only `project_id` and a caller needs the
/// canonical project slug without scanning a page-limited organization list.
pub async fn project_by_id(
    context: &ReadContext,
    project_id: uuid::Uuid,
) -> Result<Option<project::Model>, OrmError> {
    project_by_id_query(project_id)
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)
}

fn project_by_id_query(project_id: uuid::Uuid) -> Select<project::Entity> {
    project::Entity::find_by_id(project_id).filter(project::Column::IsSoftDeleted.eq(false))
}

fn project_by_org_and_slug_query(org_id: uuid::Uuid, slug: &str) -> Select<project::Entity> {
    project::Entity::find()
        .filter(project::Column::OrgId.eq(org_id))
        .filter(project::Column::Slug.eq(slug))
        .filter(project::Column::IsSoftDeleted.eq(false))
}

/// Projects the user is a direct member of, across every org.
pub async fn projects_for_user(
    context: &ReadContext,
    user_id: uuid::Uuid,
) -> Result<Vec<ProjectSummary>, OrmError> {
    let rows = project::Entity::find()
        .join(JoinType::InnerJoin, project::Relation::Org.def())
        .join(JoinType::InnerJoin, project::Relation::ProjectMember.def())
        .filter(project_member::Column::UserId.eq(user_id))
        .filter(project::Column::IsSoftDeleted.eq(false))
        .select_also(org::Entity)
        .order_by_asc(project::Column::Slug)
        .limit(PAGE_LIMIT)
        .all(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;

    Ok(rows
        .into_iter()
        .map(|(row, org_row)| ProjectSummary {
            id: row.id,
            org_id: row.org_id,
            org_slug: org_row.map(|value| value.slug).unwrap_or_default(),
            slug: row.slug,
            name: row.name,
            description: row.description,
            role: String::new(),
        })
        .collect())
}

/// The viewer's direct role in a project, or `None` if they are not a direct
/// project member.
///
/// Organization membership remains a separate authority. Callers that accept
/// either scope must combine this result with [`org_role_for_user`] using the
/// same role precedence as the account-control plane. This exact composite-key
/// lookup intentionally does not scan the paginated [`projects_for_user`]
/// listing.
pub async fn project_role_for_user(
    context: &ReadContext,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<Option<String>, OrmError> {
    let membership = project_role_for_user_query(project_id, user_id)
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(membership.map(|row| row.role))
}

fn project_role_for_user_query(
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Select<project_member::Entity> {
    project_member::Entity::find_by_id((project_id, user_id))
}

fn package_summary(row: package::Model, org_slug: String) -> PackageSummary {
    PackageSummary {
        id: row.id,
        org_id: row.org_id,
        org_slug,
        project_id: row.project_id,
        project_slug: None,
        name: row.name,
        description: row.description,
        visibility: row.visibility,
        repo_url: row.repo_url,
        config: row.config,
        latest_version: row.latest_version,
        download_count: row.download_count,
        version_count: row.version_count,
    }
}

/// Packages in an org. `include_private` must reflect a checked membership.
pub async fn packages_for_org(
    context: &ReadContext,
    org_id: uuid::Uuid,
    org_slug: &str,
    include_private: bool,
) -> Result<Vec<PackageSummary>, OrmError> {
    let mut query = package::Entity::find()
        .filter(package::Column::OrgId.eq(org_id))
        .filter(package::Column::IsSoftDeleted.eq(false));
    if !include_private {
        query = query.filter(package::Column::Visibility.eq("public"));
    }

    let rows = query
        .order_by_asc(package::Column::Name)
        .limit(PAGE_LIMIT)
        .all(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;

    Ok(rows
        .into_iter()
        .map(|row| package_summary(row, org_slug.to_owned()))
        .collect())
}

/// Packages filed under one project.
pub async fn packages_for_project(
    context: &ReadContext,
    project_id: uuid::Uuid,
    org_slug: &str,
) -> Result<Vec<PackageSummary>, OrmError> {
    let rows = package::Entity::find()
        .filter(package::Column::ProjectId.eq(project_id))
        .filter(package::Column::IsSoftDeleted.eq(false))
        .order_by_asc(package::Column::Name)
        .limit(PAGE_LIMIT)
        .all(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;

    Ok(rows
        .into_iter()
        .map(|row| package_summary(row, org_slug.to_owned()))
        .collect())
}

/// A single package addressed the way its URL addresses it: `{org}/{name}`.
pub async fn package_by_org_and_name(
    context: &ReadContext,
    org_slug: &str,
    name: &str,
) -> Result<Option<(package::Model, org::Model)>, OrmError> {
    let Some(org_row) = org_by_slug(context, org_slug).await? else {
        return Ok(None);
    };
    let found = package::Entity::find()
        .filter(package::Column::OrgId.eq(org_row.id))
        .filter(package::Column::Name.eq(name))
        .filter(package::Column::IsSoftDeleted.eq(false))
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(found.map(|row| (row, org_row)))
}

/// Published versions, newest first. Yanked versions are included: the package
/// page shows them struck through rather than hiding history.
pub async fn versions_for_package(
    context: &ReadContext,
    package_id: uuid::Uuid,
) -> Result<Vec<package_version::Model>, OrmError> {
    package_version::Entity::find()
        .filter(package_version::Column::PackageId.eq(package_id))
        .order_by_desc(package_version::Column::PublishedAt)
        .limit(PAGE_LIMIT)
        .all(context.connection())
        .await
        .map_err(OrmError::from_db_err)
}

/// One immutable published version addressed by its package and exact version.
///
/// This is the read path for exact-version downloads and dependency graphs. It
/// deliberately avoids the page-limited [`versions_for_package`] listing so an
/// old but still-addressable version cannot disappear after 100 releases.
pub async fn package_version_by_package_and_version(
    context: &ReadContext,
    package_id: uuid::Uuid,
    version: &str,
) -> Result<Option<package_version::Model>, OrmError> {
    package_version_by_package_and_version_query(package_id, version)
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)
}

fn package_version_by_package_and_version_query(
    package_id: uuid::Uuid,
    version: &str,
) -> Select<package_version::Entity> {
    package_version::Entity::find()
        .filter(package_version::Column::PackageId.eq(package_id))
        .filter(package_version::Column::Version.eq(version))
}

/// Licenses for a package: the package-level default plus any version overrides.
pub async fn licenses_for_package(
    context: &ReadContext,
    package_id: uuid::Uuid,
) -> Result<Vec<package_license::Model>, OrmError> {
    package_license::Entity::find()
        .filter(package_license::Column::PackageId.eq(package_id))
        .order_by_asc(package_license::Column::PackageVersionId)
        .all(context.connection())
        .await
        .map_err(OrmError::from_db_err)
}

/// Newest public packages — the signed-out home page.
pub async fn recent_public_packages(
    context: &ReadContext,
    limit: u64,
) -> Result<Vec<PackageSummary>, OrmError> {
    let rows = package::Entity::find()
        .filter(package::Column::Visibility.eq("public"))
        .filter(package::Column::IsSoftDeleted.eq(false))
        .find_also_related(org::Entity)
        .order_by_desc(package::Column::CreatedAt)
        .limit(limit.min(PAGE_LIMIT))
        .all(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;

    Ok(rows
        .into_iter()
        .map(|(row, org_row)| {
            let slug = org_row.map(|value| value.slug).unwrap_or_default();
            package_summary(row, slug)
        })
        .collect())
}

/// Substring search over public packages, plus private packages in the orgs the
/// viewer belongs to.
///
/// `visible_org_ids` is supplied by the caller rather than derived here, so the
/// authorization decision stays in one place and this stays a pure query.
pub async fn search_packages(
    context: &ReadContext,
    query: &str,
    visible_org_ids: &[uuid::Uuid],
    limit: u64,
) -> Result<Vec<PackageSummary>, OrmError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = format!("%{}%", trimmed.to_lowercase());

    let mut visibility = Condition::any().add(package::Column::Visibility.eq("public"));
    if !visible_org_ids.is_empty() {
        visibility = visibility.add(package::Column::OrgId.is_in(visible_org_ids.to_vec()));
    }

    let rows = package::Entity::find()
        .filter(package::Column::IsSoftDeleted.eq(false))
        .filter(visibility)
        .filter(
            Condition::any()
                .add(package::Column::Name.like(&pattern))
                .add(package::Column::Description.like(&pattern)),
        )
        .find_also_related(org::Entity)
        .order_by_desc(package::Column::DownloadCount)
        .limit(limit.min(PAGE_LIMIT))
        .all(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;

    Ok(rows
        .into_iter()
        .map(|(row, org_row)| {
            let slug = org_row.map(|value| value.slug).unwrap_or_default();
            package_summary(row, slug)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use sea_orm::{DatabaseBackend, QueryTrait};

    use super::*;

    #[test]
    fn direct_project_role_lookup_uses_the_exact_composite_key() {
        let project_id = uuid::Uuid::parse_str("10000000-0000-0000-0000-000000000001")
            .expect("fixture project id");
        let user_id =
            uuid::Uuid::parse_str("20000000-0000-0000-0000-000000000002").expect("fixture user id");
        let statement =
            project_role_for_user_query(project_id, user_id).build(DatabaseBackend::Postgres);

        assert!(statement
            .sql
            .contains("\"zed_project_members\".\"project_id\" = $1"));
        assert!(statement
            .sql
            .contains("\"zed_project_members\".\"user_id\" = $2"));
        assert!(!statement.sql.contains("JOIN"));
        assert!(!statement.sql.contains(&format!("LIMIT {PAGE_LIMIT}")));
    }

    #[test]
    fn project_coordinate_lookup_uses_the_active_unique_key() {
        let org_id =
            uuid::Uuid::parse_str("30000000-0000-0000-0000-000000000003").expect("fixture org id");
        let statement =
            project_by_org_and_slug_query(org_id, "compiler").build(DatabaseBackend::Postgres);

        assert!(statement.sql.contains("\"zed_projects\".\"org_id\" = $1"));
        assert!(statement.sql.contains("\"zed_projects\".\"slug\" = $2"));
        assert!(statement
            .sql
            .contains("\"zed_projects\".\"is_soft_deleted\" = $3"));
        assert!(!statement.sql.contains("JOIN"));
        assert!(!statement.sql.contains(&format!("LIMIT {PAGE_LIMIT}")));
    }

    #[test]
    fn project_primary_key_lookup_is_exact_and_active_only() {
        let project_id =
            uuid::Uuid::parse_str("35000000-0000-0000-0000-000000000003").expect("project id");
        let statement = project_by_id_query(project_id).build(DatabaseBackend::Postgres);

        assert!(statement.sql.contains("\"zed_projects\".\"id\" = $1"));
        assert!(statement
            .sql
            .contains("\"zed_projects\".\"is_soft_deleted\" = $2"));
        assert!(!statement.sql.contains("JOIN"));
        assert!(!statement.sql.contains(&format!("LIMIT {PAGE_LIMIT}")));
    }

    #[test]
    fn exact_package_version_lookup_does_not_scan_the_listing() {
        let package_id =
            uuid::Uuid::parse_str("40000000-0000-0000-0000-000000000004").expect("package id");
        let statement = package_version_by_package_and_version_query(package_id, "1.2.3")
            .build(DatabaseBackend::Postgres);

        assert!(statement
            .sql
            .contains("\"zed_package_versions\".\"package_id\" = $1"));
        assert!(statement
            .sql
            .contains("\"zed_package_versions\".\"version\" = $2"));
        assert!(!statement.sql.contains("JOIN"));
        assert!(!statement.sql.contains(&format!("LIMIT {PAGE_LIMIT}")));
    }

    #[tokio::test]
    #[ignore = "requires a dedicated ORM_CORE_TEST_DATABASE_URL database"]
    async fn live_direct_project_role_lookup_accepts_a_missing_membership() {
        let database_url = std::env::var("ORM_CORE_TEST_DATABASE_URL")
            .expect("ORM_CORE_TEST_DATABASE_URL must target a disposable test database");
        let context = crate::connect_read_only(&database_url)
            .await
            .expect("read-only connection must verify");

        let role = project_role_for_user(&context, uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
            .await
            .expect("exact membership lookup must execute");

        assert_eq!(role, None);
    }

    #[tokio::test]
    #[ignore = "requires a dedicated ORM_CORE_TEST_DATABASE_URL database"]
    async fn live_project_coordinate_lookup_accepts_a_missing_project() {
        let database_url = std::env::var("ORM_CORE_TEST_DATABASE_URL")
            .expect("ORM_CORE_TEST_DATABASE_URL must target a disposable test database");
        let context = crate::connect_read_only(&database_url)
            .await
            .expect("read-only connection must verify");

        let project =
            project_by_org_and_slug(&context, uuid::Uuid::new_v4(), "missing-project-probe")
                .await
                .expect("exact project lookup must execute");

        assert_eq!(project, None);
    }
}
