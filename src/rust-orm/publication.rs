//! Transactional adoption of the machine registry's immutable publish facts.
//!
//! The legacy `/v1` machine API remains available during the cutover, but a
//! successful publish must also become visible in the canonical `zed_*` data
//! plane used by `app.zpkg.net`. This module mirrors the organization, package,
//! version, verified R2 upload, and audit fact in one transaction. Replaying the
//! same immutable publish is idempotent; replaying the same version with
//! different artifact facts is rejected.

use sea_orm::{
    prelude::{Json, Uuid},
    ActiveModelTrait,
    ActiveValue::Set,
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Statement, TransactionTrait, Value,
};

use crate::{
    entities::{audit_log, org, package, package_upload, package_version},
    OrmError, WriteContext,
};

const VERSION_SCHEMES: &[&str] = &["semver", "calver", "opaque"];
const ARCHIVE_FORMATS: &[&str] = &["tar.gz", "tar.zst", "zip"];
const PACKAGE_VCS: &[&str] = &["git", "hg", "svn", "fossil"];

#[derive(Clone, Debug, PartialEq)]
pub struct MachinePublishInput {
    pub org_slug: String,
    pub org_name: Option<String>,
    pub package_name: String,
    pub description: Option<String>,
    pub vcs: String,
    pub repo_url: String,
    pub homepage_url: Option<String>,
    pub keywords: Json,
    pub version: String,
    pub version_scheme: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub format: String,
    pub vcs_tag: Option<String>,
    pub vcs_commit: Option<String>,
    pub artifact_key: String,
    pub manifest: Json,
    pub published_by_user_id: Option<Uuid>,
    pub api_token_id: Option<Uuid>,
    pub client_ip_hash: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachinePublishReceipt {
    pub org_id: Uuid,
    pub package_id: Uuid,
    pub package_version_id: Uuid,
    /// True only when this invocation inserted the immutable version row.
    pub inserted: bool,
}

/// Adopt one already-stored machine publication into the canonical control
/// plane. The caller invokes this before reporting success to the machine
/// client; failure therefore cannot produce a successful `/v1` response with a
/// missing account-console projection.
pub async fn adopt_machine_publish(
    context: &WriteContext,
    input: MachinePublishInput,
) -> Result<MachinePublishReceipt, OrmError> {
    validate(&input)?;
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;

    // Serialize first-publish adoption by the public package coordinate. This
    // covers the partial unique indexes on active orgs/packages without relying
    // on database-error text or a find-then-insert race.
    let lock_coordinate = format!(
        "zed-machine-publish:{}/{}",
        input.org_slug, input.package_name
    );
    transaction
        .execute(Statement::from_sql_and_values(
            transaction.get_database_backend(),
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            [Value::String(Some(Box::new(lock_coordinate)))],
        ))
        .await
        .map_err(OrmError::from_db_err)?;

    let organization = match org::Entity::find()
        .filter(org::Column::Slug.eq(&input.org_slug))
        .filter(org::Column::IsSoftDeleted.eq(false))
        .one(&transaction)
        .await
        .map_err(OrmError::from_db_err)?
    {
        Some(organization) => organization,
        None => {
            let now = chrono::Utc::now().fixed_offset();
            org::ActiveModel {
                id: Set(Uuid::new_v4()),
                slug: Set(input.org_slug.clone()),
                name: Set(input
                    .org_name
                    .clone()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| input.org_slug.clone())),
                description: Set(None),
                settings: Set(serde_json::json!({
                    "adopted_from": "machine_registry"
                })),
                created_by_user_id: Set(input.published_by_user_id),
                is_soft_deleted: Set(false),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&transaction)
            .await
            .map_err(OrmError::from_db_err)?
        }
    };

    let package = match package::Entity::find()
        .filter(package::Column::OrgId.eq(organization.id))
        .filter(package::Column::Name.eq(&input.package_name))
        .filter(package::Column::IsSoftDeleted.eq(false))
        .one(&transaction)
        .await
        .map_err(OrmError::from_db_err)?
    {
        Some(package) => {
            let mut active: package::ActiveModel = package.into();
            active.description = Set(input.description.clone());
            active.vcs = Set(input.vcs.clone());
            active.repo_url = Set(input.repo_url.clone());
            active.homepage_url = Set(input.homepage_url.clone());
            active.keywords = Set(input.keywords.clone());
            active
                .update(&transaction)
                .await
                .map_err(OrmError::from_db_err)?
        }
        None => {
            let now = chrono::Utc::now().fixed_offset();
            package::ActiveModel {
                id: Set(Uuid::new_v4()),
                org_id: Set(organization.id),
                project_id: Set(None),
                name: Set(input.package_name.clone()),
                description: Set(input.description.clone()),
                // The legacy machine registry is a public registry. A package
                // that was already created privately retains its visibility in
                // the branch above; only a first machine publication starts
                // public.
                visibility: Set("public".to_owned()),
                vcs: Set(input.vcs.clone()),
                repo_url: Set(input.repo_url.clone()),
                homepage_url: Set(input.homepage_url.clone()),
                keywords: Set(input.keywords.clone()),
                config: Set(serde_json::json!({
                    "adopted_from": "machine_registry",
                    "default_archive_format": input.format,
                })),
                download_count: Set(0),
                version_count: Set(0),
                latest_version: Set(None),
                first_published_at: Set(None),
                visibility_changed_at: Set(Some(now)),
                created_by_user_id: Set(input.published_by_user_id),
                is_soft_deleted: Set(false),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&transaction)
            .await
            .map_err(OrmError::from_db_err)?
        }
    };

    let existing = package_version::Entity::find()
        .filter(package_version::Column::PackageId.eq(package.id))
        .filter(package_version::Column::Version.eq(&input.version))
        .one(&transaction)
        .await
        .map_err(OrmError::from_db_err)?;

    let (version, inserted) = match existing {
        Some(version) => {
            if !immutable_facts_match(&version, &input) {
                return Err(OrmError::policy(format!(
                    "canonical package version {}/{}@{} already exists with different immutable artifact facts",
                    input.org_slug, input.package_name, input.version
                )));
            }
            (version, false)
        }
        None => {
            let version = package_version::ActiveModel {
                id: Set(Uuid::new_v4()),
                package_id: Set(package.id),
                version: Set(input.version.clone()),
                version_scheme: Set(input.version_scheme.clone()),
                sha256: Set(input.sha256.clone()),
                size_bytes: Set(input.size_bytes),
                format: Set(input.format.clone()),
                vcs_tag: Set(input.vcs_tag.clone()),
                vcs_commit: Set(input.vcs_commit.clone()),
                artifact_key: Set(input.artifact_key.clone()),
                manifest: Set(input.manifest.clone()),
                download_count: Set(0),
                yanked: Set(false),
                yanked_at: Set(None),
                yanked_reason: Set(None),
                published_by_user_id: Set(input.published_by_user_id),
                published_at: Set(chrono::Utc::now().fixed_offset()),
            }
            .insert(&transaction)
            .await
            .map_err(OrmError::from_db_err)?;
            (version, true)
        }
    };

    // A retry may encounter a version committed before the verified-upload fact
    // was written. Backfill exactly one canonical verified ledger row.
    let verified_upload = package_upload::Entity::find()
        .filter(package_upload::Column::PackageVersionId.eq(version.id))
        .filter(package_upload::Column::Status.eq("verified"))
        .one(&transaction)
        .await
        .map_err(OrmError::from_db_err)?;
    if verified_upload.is_none() {
        let now = chrono::Utc::now().fixed_offset();
        package_upload::ActiveModel {
            id: Set(Uuid::new_v4()),
            package_id: Set(package.id),
            package_version_id: Set(Some(version.id)),
            requested_version: Set(input.version.clone()),
            status: Set("verified".to_owned()),
            storage_backend: Set("r2".to_owned()),
            storage_key: Set(Some(input.artifact_key.clone())),
            format: Set(Some(input.format.clone())),
            size_bytes: Set(Some(input.size_bytes)),
            sha256: Set(Some(input.sha256.clone())),
            uploaded_by_user_id: Set(input.published_by_user_id),
            api_token_id: Set(input.api_token_id),
            client_ip_hash: Set(input.client_ip_hash.clone()),
            user_agent: Set(input.user_agent.clone()),
            error: Set(None),
            started_at: Set(now),
            completed_at: Set(Some(now)),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&transaction)
        .await
        .map_err(OrmError::from_db_err)?;
    }

    if inserted {
        audit_log::ActiveModel {
            id: Set(Uuid::new_v4()),
            org_id: Set(Some(organization.id)),
            actor_user_id: Set(input.published_by_user_id),
            api_token_id: Set(input.api_token_id),
            action: Set("package.publish.mirror".to_owned()),
            entity_type: Set("package_version".to_owned()),
            entity_id: Set(Some(version.id)),
            detail: Set(serde_json::json!({
                "package": input.package_name,
                "version": input.version,
                "sha256": input.sha256,
                "artifact_key": input.artifact_key,
                "source": "legacy_machine_api"
            })),
            client_ip_hash: Set(input.client_ip_hash),
            created_at: Set(chrono::Utc::now().fixed_offset()),
        }
        .insert(&transaction)
        .await
        .map_err(OrmError::from_db_err)?;
    }

    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(MachinePublishReceipt {
        org_id: organization.id,
        package_id: package.id,
        package_version_id: version.id,
        inserted,
    })
}

fn immutable_facts_match(version: &package_version::Model, input: &MachinePublishInput) -> bool {
    version.version_scheme == input.version_scheme
        && version.sha256 == input.sha256
        && version.size_bytes == input.size_bytes
        && version.format == input.format
        && version.vcs_tag == input.vcs_tag
        && version.vcs_commit == input.vcs_commit
        && version.artifact_key == input.artifact_key
        && version.manifest == input.manifest
}

fn validate(input: &MachinePublishInput) -> Result<(), OrmError> {
    required_text("organization slug", &input.org_slug, 64)?;
    optional_text("organization name", input.org_name.as_deref(), 200)?;
    required_text("package name", &input.package_name, 128)?;
    optional_text("package description", input.description.as_deref(), 4_096)?;
    one_of("package VCS", &input.vcs, PACKAGE_VCS)?;
    optional_text("repository URL", Some(&input.repo_url), 2_048)?;
    optional_text("homepage URL", input.homepage_url.as_deref(), 2_048)?;
    if !input.keywords.is_array() {
        return Err(OrmError::policy("package keywords must be a JSON array"));
    }
    required_text("package version", &input.version, 128)?;
    one_of("version scheme", &input.version_scheme, VERSION_SCHEMES)?;
    sha256("artifact SHA-256", &input.sha256)?;
    if input.size_bytes < 0 {
        return Err(OrmError::policy("artifact size cannot be negative"));
    }
    one_of("archive format", &input.format, ARCHIVE_FORMATS)?;
    optional_text("VCS tag", input.vcs_tag.as_deref(), 160)?;
    optional_text("VCS commit", input.vcs_commit.as_deref(), 120)?;
    required_text("R2 artifact key", &input.artifact_key, 1_024)?;
    if !input.manifest.is_object() {
        return Err(OrmError::policy("package manifest must be a JSON object"));
    }
    if let Some(hash) = input.client_ip_hash.as_deref() {
        sha256("client IP hash", hash)?;
    }
    optional_text("user agent", input.user_agent.as_deref(), 512)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> MachinePublishInput {
        MachinePublishInput {
            org_slug: "zed-pkg".to_owned(),
            org_name: Some("Zed Package Registry".to_owned()),
            package_name: "zed-lib-core".to_owned(),
            description: Some("canonical data plane".to_owned()),
            vcs: "git".to_owned(),
            repo_url: "https://github.com/zed-pkg/zed-lib-core".to_owned(),
            homepage_url: None,
            keywords: serde_json::json!(["zed", "registry"]),
            version: "0.1.0".to_owned(),
            version_scheme: "semver".to_owned(),
            sha256: "a".repeat(64),
            size_bytes: 42,
            format: "tar.gz".to_owned(),
            vcs_tag: Some("v0.1.0".to_owned()),
            vcs_commit: Some("b".repeat(40)),
            artifact_key: "sha256/aa/artifact.tar.gz".to_owned(),
            manifest: serde_json::json!({"package": {"name": "zed-lib-core"}}),
            published_by_user_id: None,
            api_token_id: None,
            client_ip_hash: None,
            user_agent: Some("zed-cli/test".to_owned()),
        }
    }

    #[test]
    fn canonical_publish_requires_complete_r2_facts() {
        assert!(validate(&input()).is_ok());
        let mut invalid = input();
        invalid.sha256 = "not-a-hash".to_owned();
        assert!(validate(&invalid).is_err());
        invalid = input();
        invalid.manifest = serde_json::json!([]);
        assert!(validate(&invalid).is_err());
    }

    #[test]
    fn immutable_fact_comparison_detects_drift() {
        let input = input();
        let now = chrono::Utc::now().fixed_offset();
        let version = package_version::Model {
            id: Uuid::nil(),
            package_id: Uuid::nil(),
            version: input.version.clone(),
            version_scheme: input.version_scheme.clone(),
            sha256: input.sha256.clone(),
            size_bytes: input.size_bytes,
            format: input.format.clone(),
            vcs_tag: input.vcs_tag.clone(),
            vcs_commit: input.vcs_commit.clone(),
            artifact_key: input.artifact_key.clone(),
            manifest: input.manifest.clone(),
            download_count: 0,
            yanked: false,
            yanked_at: None,
            yanked_reason: None,
            published_by_user_id: None,
            published_at: now,
        };
        assert!(immutable_facts_match(&version, &input));
        let mut changed = input;
        changed.size_bytes += 1;
        assert!(!immutable_facts_match(&version, &changed));
    }
}
