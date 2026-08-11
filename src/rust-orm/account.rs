//! Transactional account-control operations for the registry API.
//!
//! These operations deliberately combine authorization and mutation inside one
//! database transaction. The API tier supplies a verified Shared Auth subject;
//! this module resolves the corresponding `zed_users` row, rechecks the
//! relevant membership, and performs the write without a time-of-check /
//! time-of-use gap.

use sea_orm::{
    prelude::{DateTimeWithTimeZone, Json, Uuid},
    sea_query::Expr,
    ActiveModelTrait,
    ActiveValue::Set,
    ColumnTrait, ConnectionTrait, DatabaseTransaction, EntityTrait, QueryFilter, TransactionTrait,
};

use crate::{
    entities::{
        org, org_invitation, org_member, package, package_license, package_upload, package_version,
        project, project_invitation, project_member,
    },
    models::InvitationReceipt,
    OrmError, WriteContext,
};

// Preserve the read-write account surface while keeping this read-only lookup
// available to default consumers through `zed_orm_core::read`.
pub use crate::read::{project_by_org_and_slug, project_role_for_user};

const ADMIN_ROLES: &[&str] = &["owner", "admin"];
const WRITE_ROLES: &[&str] = &["owner", "admin", "member"];
const INVITABLE_ROLES: &[&str] = &["admin", "member", "reader"];
const PROJECT_VISIBILITIES: &[&str] = &["private", "internal", "public"];
const PACKAGE_VCS: &[&str] = &["git", "hg", "svn", "fossil"];
const LICENSE_KINDS: &[&str] = &["spdx", "custom", "proprietary"];
const UPLOAD_STATUSES: &[&str] = &[
    "pending",
    "uploading",
    "stored",
    "verified",
    "failed",
    "aborted",
];
const STORAGE_BACKENDS: &[&str] = &["s3", "r2", "gcs", "fs"];
const ARCHIVE_FORMATS: &[&str] = &["tar.gz", "tar.zst", "zip"];

#[derive(Clone, Debug, PartialEq)]
pub struct CreateProjectInput {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub settings: Json,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreatePackageInput {
    pub project_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub vcs: String,
    pub repo_url: String,
    pub homepage_url: Option<String>,
    pub keywords: Json,
    pub config: Json,
}

/// General package settings. Visibility is intentionally absent: promotion is
/// a dedicated guarded operation and cannot be smuggled through this patch.
#[derive(Clone, Debug, PartialEq)]
pub struct PackageSettingsPatch {
    pub description: Option<String>,
    pub project_id: Option<Uuid>,
    pub repo_url: String,
    pub homepage_url: Option<String>,
    pub keywords: Json,
    pub config: Json,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvitationInput {
    pub email: String,
    pub role: String,
    /// One-time plaintext returned to the caller; never persisted.
    pub token: String,
    /// Lowercase hexadecimal SHA-256 of `token`.
    pub token_hash: String,
    pub expires_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PackageLicenseInput {
    pub package_version_id: Option<Uuid>,
    pub kind: String,
    pub spdx_id: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
    pub text_body: Option<String>,
    pub is_primary: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageUploadInput {
    pub package_version_id: Option<Uuid>,
    pub requested_version: String,
    pub status: String,
    pub storage_backend: String,
    pub storage_key: Option<String>,
    pub format: Option<String>,
    pub size_bytes: Option<i64>,
    pub sha256: Option<String>,
    pub api_token_id: Option<Uuid>,
    pub client_ip_hash: Option<String>,
    pub user_agent: Option<String>,
    pub error: Option<String>,
    pub completed_at: Option<DateTimeWithTimeZone>,
}

/// Create a project after rechecking the actor's organization-admin role in the
/// same transaction that inserts the project and owner membership.
pub async fn create_project_for_user(
    context: &WriteContext,
    actor_user_id: Uuid,
    org_slug: &str,
    input: CreateProjectInput,
) -> Result<project::Model, OrmError> {
    validate_project(&input)?;
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;
    let organization = require_org_role(&transaction, actor_user_id, org_slug, ADMIN_ROLES).await?;
    let now = chrono::Utc::now().fixed_offset();
    let project = project::ActiveModel {
        id: Set(Uuid::new_v4()),
        org_id: Set(organization.id),
        slug: Set(input.slug),
        name: Set(input.name),
        description: Set(input.description),
        visibility: Set(input.visibility),
        settings: Set(input.settings),
        created_by_user_id: Set(Some(actor_user_id)),
        is_soft_deleted: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;
    project_member::ActiveModel {
        project_id: Set(project.id),
        user_id: Set(actor_user_id),
        role: Set("owner".to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;
    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(project)
}

pub async fn invite_org_member_for_user(
    context: &WriteContext,
    actor_user_id: Uuid,
    org_slug: &str,
    input: InvitationInput,
) -> Result<InvitationReceipt, OrmError> {
    validate_invitation(&input)?;
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;
    let organization = require_org_role(&transaction, actor_user_id, org_slug, ADMIN_ROLES).await?;
    let invitation_id = Uuid::new_v4();
    org_invitation::ActiveModel {
        id: Set(invitation_id),
        org_id: Set(organization.id),
        invited_by_user_id: Set(actor_user_id),
        email: Set(normalize_email(&input.email)?),
        role: Set(input.role.clone()),
        token_hash: Set(input.token_hash),
        expires_at: Set(input.expires_at),
        accepted_at: Set(None),
        accepted_by_user_id: Set(None),
        revoked_at: Set(None),
        created_at: Set(chrono::Utc::now().fixed_offset()),
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;
    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(InvitationReceipt {
        invitation_id,
        token: input.token,
        email: normalize_email(&input.email)?,
        role: input.role,
    })
}

pub async fn invite_project_member_for_user(
    context: &WriteContext,
    actor_user_id: Uuid,
    org_slug: &str,
    project_slug: &str,
    input: InvitationInput,
) -> Result<InvitationReceipt, OrmError> {
    validate_invitation(&input)?;
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;
    let organization = find_org(&transaction, org_slug).await?;
    let project = find_project(&transaction, organization.id, project_slug).await?;
    require_project_role(
        &transaction,
        actor_user_id,
        &organization,
        &project,
        ADMIN_ROLES,
    )
    .await?;
    let invitation_id = Uuid::new_v4();
    let email = normalize_email(&input.email)?;
    project_invitation::ActiveModel {
        id: Set(invitation_id),
        project_id: Set(project.id),
        invited_by_user_id: Set(actor_user_id),
        email: Set(email.clone()),
        role: Set(input.role.clone()),
        token_hash: Set(input.token_hash),
        expires_at: Set(input.expires_at),
        accepted_at: Set(None),
        accepted_by_user_id: Set(None),
        revoked_at: Set(None),
        created_at: Set(chrono::Utc::now().fixed_offset()),
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;
    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(InvitationReceipt {
        invitation_id,
        token: input.token,
        email,
        role: input.role,
    })
}

pub async fn create_package_for_user(
    context: &WriteContext,
    actor_user_id: Uuid,
    org_slug: &str,
    input: CreatePackageInput,
) -> Result<package::Model, OrmError> {
    validate_package_create(&input)?;
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;
    let organization = find_org(&transaction, org_slug).await?;
    require_package_write_target(
        &transaction,
        actor_user_id,
        &organization,
        input.project_id,
        WRITE_ROLES,
    )
    .await?;
    let now = chrono::Utc::now().fixed_offset();
    let package = package::ActiveModel {
        id: Set(Uuid::new_v4()),
        org_id: Set(organization.id),
        project_id: Set(input.project_id),
        name: Set(input.name),
        description: Set(input.description),
        visibility: Set("private".to_owned()),
        vcs: Set(input.vcs),
        repo_url: Set(input.repo_url),
        homepage_url: Set(input.homepage_url),
        keywords: Set(input.keywords),
        config: Set(input.config),
        download_count: Set(0),
        version_count: Set(0),
        latest_version: Set(None),
        first_published_at: Set(None),
        visibility_changed_at: Set(None),
        created_by_user_id: Set(Some(actor_user_id)),
        is_soft_deleted: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;
    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(package)
}

pub async fn update_package_settings_for_user(
    context: &WriteContext,
    actor_user_id: Uuid,
    org_slug: &str,
    package_name: &str,
    patch: PackageSettingsPatch,
) -> Result<package::Model, OrmError> {
    validate_package_patch(&patch)?;
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;
    let organization = find_org(&transaction, org_slug).await?;
    let package = find_package(&transaction, organization.id, package_name).await?;
    require_package_role(
        &transaction,
        actor_user_id,
        &organization,
        &package,
        WRITE_ROLES,
    )
    .await?;
    require_package_write_target(
        &transaction,
        actor_user_id,
        &organization,
        patch.project_id,
        WRITE_ROLES,
    )
    .await?;

    let mut active: package::ActiveModel = package.into();
    active.description = Set(patch.description);
    active.project_id = Set(patch.project_id);
    active.repo_url = Set(patch.repo_url);
    active.homepage_url = Set(patch.homepage_url);
    active.keywords = Set(patch.keywords);
    active.config = Set(patch.config);
    let updated = active
        .update(&transaction)
        .await
        .map_err(OrmError::from_db_err)?;
    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(updated)
}

pub async fn make_package_public_for_user(
    context: &WriteContext,
    actor_user_id: Uuid,
    org_slug: &str,
    package_name: &str,
) -> Result<package::Model, OrmError> {
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;
    let organization = find_org(&transaction, org_slug).await?;
    let package = find_package(&transaction, organization.id, package_name).await?;
    require_package_role(
        &transaction,
        actor_user_id,
        &organization,
        &package,
        ADMIN_ROLES,
    )
    .await?;
    if package.visibility == "public" {
        transaction.commit().await.map_err(OrmError::from_db_err)?;
        return Ok(package);
    }
    let mut active: package::ActiveModel = package.into();
    active.visibility = Set("public".to_owned());
    let updated = active
        .update(&transaction)
        .await
        .map_err(OrmError::from_db_err)?;
    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(updated)
}

pub async fn add_package_license_for_user(
    context: &WriteContext,
    actor_user_id: Uuid,
    org_slug: &str,
    package_name: &str,
    input: PackageLicenseInput,
) -> Result<package_license::Model, OrmError> {
    validate_license(&input)?;
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;
    let organization = find_org(&transaction, org_slug).await?;
    let package = find_package(&transaction, organization.id, package_name).await?;
    require_package_role(
        &transaction,
        actor_user_id,
        &organization,
        &package,
        WRITE_ROLES,
    )
    .await?;
    require_version_belongs_to_package(&transaction, package.id, input.package_version_id).await?;

    if input.is_primary {
        let mut demote = package_license::Entity::update_many()
            .col_expr(package_license::Column::IsPrimary, Expr::value(false))
            .filter(package_license::Column::PackageId.eq(package.id))
            .filter(package_license::Column::IsPrimary.eq(true));
        demote = match input.package_version_id {
            Some(version_id) => {
                demote.filter(package_license::Column::PackageVersionId.eq(version_id))
            }
            None => demote.filter(package_license::Column::PackageVersionId.is_null()),
        };
        demote
            .exec(&transaction)
            .await
            .map_err(OrmError::from_db_err)?;
    }

    let now = chrono::Utc::now().fixed_offset();
    let license = package_license::ActiveModel {
        id: Set(Uuid::new_v4()),
        package_id: Set(package.id),
        package_version_id: Set(input.package_version_id),
        kind: Set(input.kind),
        spdx_id: Set(input.spdx_id),
        name: Set(input.name),
        url: Set(input.url),
        text_body: Set(input.text_body),
        is_primary: Set(input.is_primary),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;
    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(license)
}

pub async fn register_package_upload_for_user(
    context: &WriteContext,
    actor_user_id: Uuid,
    org_slug: &str,
    package_name: &str,
    input: PackageUploadInput,
) -> Result<package_upload::Model, OrmError> {
    validate_upload(&input)?;
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;
    let organization = find_org(&transaction, org_slug).await?;
    let package = find_package(&transaction, organization.id, package_name).await?;
    require_package_role(
        &transaction,
        actor_user_id,
        &organization,
        &package,
        WRITE_ROLES,
    )
    .await?;
    require_version_belongs_to_package(&transaction, package.id, input.package_version_id).await?;

    let now = chrono::Utc::now().fixed_offset();
    let upload = package_upload::ActiveModel {
        id: Set(Uuid::new_v4()),
        package_id: Set(package.id),
        package_version_id: Set(input.package_version_id),
        requested_version: Set(input.requested_version),
        status: Set(input.status),
        storage_backend: Set(input.storage_backend),
        storage_key: Set(input.storage_key),
        format: Set(input.format),
        size_bytes: Set(input.size_bytes),
        sha256: Set(input.sha256),
        uploaded_by_user_id: Set(Some(actor_user_id)),
        api_token_id: Set(input.api_token_id),
        client_ip_hash: Set(input.client_ip_hash),
        user_agent: Set(input.user_agent),
        error: Set(input.error),
        started_at: Set(now),
        completed_at: Set(input.completed_at),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;
    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(upload)
}

async fn find_org<C: ConnectionTrait>(connection: &C, slug: &str) -> Result<org::Model, OrmError> {
    org::Entity::find()
        .filter(org::Column::Slug.eq(slug))
        .filter(org::Column::IsSoftDeleted.eq(false))
        .one(connection)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| OrmError::not_found("organization"))
}

async fn find_project<C: ConnectionTrait>(
    connection: &C,
    org_id: Uuid,
    slug: &str,
) -> Result<project::Model, OrmError> {
    project::Entity::find()
        .filter(project::Column::OrgId.eq(org_id))
        .filter(project::Column::Slug.eq(slug))
        .filter(project::Column::IsSoftDeleted.eq(false))
        .one(connection)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| OrmError::not_found("project"))
}

async fn find_package<C: ConnectionTrait>(
    connection: &C,
    org_id: Uuid,
    name: &str,
) -> Result<package::Model, OrmError> {
    package::Entity::find()
        .filter(package::Column::OrgId.eq(org_id))
        .filter(package::Column::Name.eq(name))
        .filter(package::Column::IsSoftDeleted.eq(false))
        .one(connection)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| OrmError::not_found("package"))
}

async fn require_org_role(
    transaction: &DatabaseTransaction,
    user_id: Uuid,
    org_slug: &str,
    allowed: &[&str],
) -> Result<org::Model, OrmError> {
    let organization = find_org(transaction, org_slug).await?;
    let role = org_member::Entity::find_by_id((organization.id, user_id))
        .one(transaction)
        .await
        .map_err(OrmError::from_db_err)?
        .map(|membership| membership.role)
        .ok_or_else(|| OrmError::policy("organization membership required"))?;
    require_allowed_role(&role, allowed)?;
    Ok(organization)
}

async fn require_project_role(
    transaction: &DatabaseTransaction,
    user_id: Uuid,
    organization: &org::Model,
    project: &project::Model,
    allowed: &[&str],
) -> Result<String, OrmError> {
    let org_role = org_member::Entity::find_by_id((organization.id, user_id))
        .one(transaction)
        .await
        .map_err(OrmError::from_db_err)?
        .map(|membership| membership.role);
    let project_role = project_member::Entity::find_by_id((project.id, user_id))
        .one(transaction)
        .await
        .map_err(OrmError::from_db_err)?
        .map(|membership| membership.role);
    let role = strongest_role(org_role.as_deref(), project_role.as_deref())
        .ok_or_else(|| OrmError::policy("project membership required"))?;
    require_allowed_role(role, allowed)?;
    Ok(role.to_owned())
}

async fn require_package_write_target(
    transaction: &DatabaseTransaction,
    user_id: Uuid,
    organization: &org::Model,
    project_id: Option<Uuid>,
    allowed: &[&str],
) -> Result<(), OrmError> {
    if let Some(project_id) = project_id {
        let project = project::Entity::find_by_id(project_id)
            .filter(project::Column::OrgId.eq(organization.id))
            .filter(project::Column::IsSoftDeleted.eq(false))
            .one(transaction)
            .await
            .map_err(OrmError::from_db_err)?
            .ok_or_else(|| OrmError::policy("project must belong to the selected organization"))?;
        require_project_role(transaction, user_id, organization, &project, allowed).await?;
        return Ok(());
    }
    let role = org_member::Entity::find_by_id((organization.id, user_id))
        .one(transaction)
        .await
        .map_err(OrmError::from_db_err)?
        .map(|membership| membership.role)
        .ok_or_else(|| OrmError::policy("organization membership required"))?;
    require_allowed_role(&role, allowed)
}

async fn require_package_role(
    transaction: &DatabaseTransaction,
    user_id: Uuid,
    organization: &org::Model,
    package: &package::Model,
    allowed: &[&str],
) -> Result<(), OrmError> {
    require_package_write_target(
        transaction,
        user_id,
        organization,
        package.project_id,
        allowed,
    )
    .await
}

async fn require_version_belongs_to_package(
    transaction: &DatabaseTransaction,
    package_id: Uuid,
    package_version_id: Option<Uuid>,
) -> Result<(), OrmError> {
    let Some(version_id) = package_version_id else {
        return Ok(());
    };
    let version = package_version::Entity::find_by_id(version_id)
        .one(transaction)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| OrmError::not_found("package version"))?;
    if version.package_id == package_id {
        Ok(())
    } else {
        Err(OrmError::policy(
            "package version must belong to the selected package",
        ))
    }
}

fn strongest_role<'a>(org_role: Option<&'a str>, project_role: Option<&'a str>) -> Option<&'a str> {
    [org_role, project_role]
        .into_iter()
        .flatten()
        .max_by_key(|role| role_rank(role))
}

fn role_rank(role: &str) -> u8 {
    match role {
        "owner" => 4,
        "admin" => 3,
        "member" => 2,
        "reader" => 1,
        _ => 0,
    }
}

fn require_allowed_role(role: &str, allowed: &[&str]) -> Result<(), OrmError> {
    if allowed.contains(&role) {
        Ok(())
    } else {
        Err(OrmError::policy("write-capable membership required"))
    }
}

fn validate_project(input: &CreateProjectInput) -> Result<(), OrmError> {
    required_text("project slug", &input.slug, 64)?;
    required_text("project name", &input.name, 200)?;
    optional_text("project description", input.description.as_deref(), 4_096)?;
    one_of(
        "project visibility",
        &input.visibility,
        PROJECT_VISIBILITIES,
    )?;
    json_object("project settings", &input.settings)
}

fn validate_package_create(input: &CreatePackageInput) -> Result<(), OrmError> {
    required_text("package name", &input.name, 128)?;
    optional_text("package description", input.description.as_deref(), 4_096)?;
    one_of("package VCS", &input.vcs, PACKAGE_VCS)?;
    optional_text("repository URL", Some(&input.repo_url), 2_048)?;
    optional_text("homepage URL", input.homepage_url.as_deref(), 2_048)?;
    json_array("package keywords", &input.keywords)?;
    json_object("package config", &input.config)
}

fn validate_package_patch(input: &PackageSettingsPatch) -> Result<(), OrmError> {
    optional_text("package description", input.description.as_deref(), 4_096)?;
    optional_text("repository URL", Some(&input.repo_url), 2_048)?;
    optional_text("homepage URL", input.homepage_url.as_deref(), 2_048)?;
    json_array("package keywords", &input.keywords)?;
    json_object("package config", &input.config)
}

fn validate_invitation(input: &InvitationInput) -> Result<(), OrmError> {
    normalize_email(&input.email)?;
    one_of("invitation role", &input.role, INVITABLE_ROLES)?;
    if input.token.len() < 32 {
        return Err(OrmError::policy(
            "invitation token must carry at least 128 bits of entropy",
        ));
    }
    sha256("invitation token hash", &input.token_hash)
}

fn validate_license(input: &PackageLicenseInput) -> Result<(), OrmError> {
    one_of("license kind", &input.kind, LICENSE_KINDS)?;
    optional_text("license name", input.name.as_deref(), 200)?;
    optional_text("license URL", input.url.as_deref(), 2_048)?;
    optional_text("license text", input.text_body.as_deref(), 262_144)?;
    match input.kind.as_str() {
        "spdx" => {
            let identifier = input
                .spdx_id
                .as_deref()
                .ok_or_else(|| OrmError::policy("SPDX licenses require an identifier"))?;
            if identifier.is_empty()
                || identifier.len() > 120
                || !identifier.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'(' | b')' | b'-')
                })
            {
                return Err(OrmError::policy("SPDX identifier has an invalid format"));
            }
        }
        "custom" => {
            if input.spdx_id.is_some()
                || (input.text_body.as_deref().is_none_or(str::is_empty)
                    && input.url.as_deref().is_none_or(str::is_empty))
            {
                return Err(OrmError::policy(
                    "custom licenses require text or a URL and cannot carry an SPDX id",
                ));
            }
        }
        "proprietary" => {
            if input.spdx_id.is_some() {
                return Err(OrmError::policy(
                    "proprietary licenses cannot carry an SPDX id",
                ));
            }
        }
        _ => unreachable!("license kind was validated above"),
    }
    Ok(())
}

fn validate_upload(input: &PackageUploadInput) -> Result<(), OrmError> {
    required_text("requested version", &input.requested_version, 128)?;
    one_of("upload status", &input.status, UPLOAD_STATUSES)?;
    one_of("storage backend", &input.storage_backend, STORAGE_BACKENDS)?;
    optional_text("storage key", input.storage_key.as_deref(), 1_024)?;
    optional_one_of("archive format", input.format.as_deref(), ARCHIVE_FORMATS)?;
    optional_nonnegative("upload size", input.size_bytes)?;
    optional_sha256("upload SHA-256", input.sha256.as_deref())?;
    optional_sha256("client IP hash", input.client_ip_hash.as_deref())?;
    optional_text("user agent", input.user_agent.as_deref(), 512)?;
    optional_text("upload error", input.error.as_deref(), 4_096)?;

    match input.status.as_str() {
        "stored" => {
            require_stored_evidence(input)?;
            if input.completed_at.is_some() {
                return Err(OrmError::policy(
                    "stored uploads are not terminal and cannot have completed_at",
                ));
            }
        }
        "verified" => {
            require_stored_evidence(input)?;
            if input.package_version_id.is_none() || input.completed_at.is_none() {
                return Err(OrmError::policy(
                    "verified uploads require a package version and completed_at",
                ));
            }
        }
        "failed" | "aborted" => {
            if input.package_version_id.is_some() || input.completed_at.is_none() {
                return Err(OrmError::policy(
                    "failed or aborted uploads require completed_at and no package version",
                ));
            }
        }
        "pending" | "uploading" => {
            if input.completed_at.is_some() {
                return Err(OrmError::policy(
                    "nonterminal uploads cannot have completed_at",
                ));
            }
        }
        _ => unreachable!("upload status was validated above"),
    }
    Ok(())
}

fn require_stored_evidence(input: &PackageUploadInput) -> Result<(), OrmError> {
    if input.storage_key.is_none()
        || input.format.is_none()
        || input.size_bytes.is_none()
        || input.sha256.is_none()
    {
        Err(OrmError::policy(
            "stored and verified uploads require storage key, format, size, and SHA-256",
        ))
    } else {
        Ok(())
    }
}

fn normalize_email(value: &str) -> Result<String, OrmError> {
    let email = value.trim().to_ascii_lowercase();
    if !(3..=320).contains(&email.len()) || !email.contains('@') {
        return Err(OrmError::policy("invitation email has an invalid format"));
    }
    Ok(email)
}

fn required_text(field: &str, value: &str, maximum: usize) -> Result<(), OrmError> {
    if value.trim().is_empty() || value.len() > maximum {
        Err(OrmError::policy(format!(
            "{field} is required and must be at most {maximum} bytes"
        )))
    } else {
        Ok(())
    }
}

fn optional_text(field: &str, value: Option<&str>, maximum: usize) -> Result<(), OrmError> {
    if value.is_some_and(|value| value.len() > maximum) {
        Err(OrmError::policy(format!(
            "{field} must be at most {maximum} bytes"
        )))
    } else {
        Ok(())
    }
}

fn one_of(field: &str, value: &str, allowed: &[&str]) -> Result<(), OrmError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(OrmError::policy(format!(
            "{field} must be one of {}",
            allowed.join(", ")
        )))
    }
}

fn optional_one_of(field: &str, value: Option<&str>, allowed: &[&str]) -> Result<(), OrmError> {
    match value {
        Some(value) => one_of(field, value, allowed),
        None => Ok(()),
    }
}

fn optional_nonnegative(field: &str, value: Option<i64>) -> Result<(), OrmError> {
    if value.is_some_and(|value| value < 0) {
        Err(OrmError::policy(format!("{field} cannot be negative")))
    } else {
        Ok(())
    }
}

fn sha256(field: &str, value: &str) -> Result<(), OrmError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(OrmError::policy(format!(
            "{field} must be 64 lowercase hexadecimal characters"
        )))
    }
}

fn optional_sha256(field: &str, value: Option<&str>) -> Result<(), OrmError> {
    match value {
        Some(value) => sha256(field, value),
        None => Ok(()),
    }
}

fn json_object(field: &str, value: &Json) -> Result<(), OrmError> {
    if value.is_object() {
        Ok(())
    } else {
        Err(OrmError::policy(format!("{field} must be a JSON object")))
    }
}

fn json_array(field: &str, value: &Json) -> Result<(), OrmError> {
    if value.is_array() {
        Ok(())
    } else {
        Err(OrmError::policy(format!("{field} must be a JSON array")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strongest_role_uses_the_more_powerful_scope() {
        assert_eq!(strongest_role(Some("member"), Some("admin")), Some("admin"));
        assert_eq!(strongest_role(Some("owner"), Some("reader")), Some("owner"));
        assert_eq!(strongest_role(None, Some("member")), Some("member"));
    }

    #[test]
    fn package_settings_cannot_carry_visibility() {
        let patch = PackageSettingsPatch {
            description: None,
            project_id: None,
            repo_url: String::new(),
            homepage_url: None,
            keywords: serde_json::json!([]),
            config: serde_json::json!({}),
        };
        assert!(validate_package_patch(&patch).is_ok());
    }

    #[test]
    fn project_invites_reject_owner_escalation() {
        let invitation = InvitationInput {
            email: "member@example.test".to_owned(),
            role: "owner".to_owned(),
            token: "x".repeat(64),
            token_hash: "a".repeat(64),
            expires_at: chrono::Utc::now().fixed_offset(),
        };
        assert!(validate_invitation(&invitation).is_err());
    }

    #[test]
    fn verified_uploads_require_immutable_artifact_evidence() {
        let input = PackageUploadInput {
            package_version_id: None,
            requested_version: "1.0.0".to_owned(),
            status: "verified".to_owned(),
            storage_backend: "r2".to_owned(),
            storage_key: None,
            format: None,
            size_bytes: None,
            sha256: None,
            api_token_id: None,
            client_ip_hash: None,
            user_agent: None,
            error: None,
            completed_at: None,
        };
        assert!(validate_upload(&input).is_err());
    }
}
