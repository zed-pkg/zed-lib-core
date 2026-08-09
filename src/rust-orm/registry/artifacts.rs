use sea_orm::{
    prelude::{DateTimeWithTimeZone, Uuid},
    ActiveModelTrait,
    ActiveValue::Set,
    EntityTrait,
};

use crate::{
    entities::{package, package_download, package_upload, package_version},
    OrmError, WriteContext,
};

use super::validation::{
    one_of, optional_nonnegative, optional_one_of, optional_sha256, optional_text, required_text,
};

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
const DOWNLOAD_SOURCES: &[&str] = &["cli", "web", "api", "mirror", "ci"];

/// Complete canonical publish-attempt record.
///
/// Authorization is owned by the API or worker before this command is
/// constructed. This boundary validates persistence invariants and never
/// accepts raw object bytes or storage credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageUploadInput {
    pub package_id: Uuid,
    pub package_version_id: Option<Uuid>,
    pub requested_version: String,
    pub status: String,
    pub storage_backend: String,
    pub storage_key: Option<String>,
    pub format: Option<String>,
    pub size_bytes: Option<i64>,
    pub sha256: Option<String>,
    pub uploaded_by_user_id: Option<Uuid>,
    pub api_token_id: Option<Uuid>,
    pub client_ip_hash: Option<String>,
    pub user_agent: Option<String>,
    pub error: Option<String>,
    pub completed_at: Option<DateTimeWithTimeZone>,
}

/// Record one successful artifact or metadata delivery.
///
/// Failed deliveries are not inserted: every row increments the package and
/// optional version counters through the canonical database trigger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageDownloadInput {
    pub package_id: Uuid,
    pub package_version_id: Option<Uuid>,
    pub downloaded_by_user_id: Option<Uuid>,
    pub api_token_id: Option<Uuid>,
    pub source: String,
    pub format: Option<String>,
    pub bytes_sent: Option<i64>,
    pub client_ip_hash: Option<String>,
    pub user_agent: Option<String>,
}

pub async fn register_package_upload(
    context: &WriteContext,
    input: PackageUploadInput,
) -> Result<package_upload::Model, OrmError> {
    validate_upload(&input)?;
    require_package(context, input.package_id).await?;
    require_version_belongs_to_package(context, input.package_id, input.package_version_id).await?;

    let now = chrono::Utc::now().fixed_offset();
    package_upload::ActiveModel {
        id: Set(Uuid::new_v4()),
        package_id: Set(input.package_id),
        package_version_id: Set(input.package_version_id),
        requested_version: Set(input.requested_version),
        status: Set(input.status),
        storage_backend: Set(input.storage_backend),
        storage_key: Set(input.storage_key),
        format: Set(input.format),
        size_bytes: Set(input.size_bytes),
        sha256: Set(input.sha256),
        uploaded_by_user_id: Set(input.uploaded_by_user_id),
        api_token_id: Set(input.api_token_id),
        client_ip_hash: Set(input.client_ip_hash),
        user_agent: Set(input.user_agent),
        error: Set(input.error),
        started_at: Set(now),
        completed_at: Set(input.completed_at),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(context.connection())
    .await
    .map_err(OrmError::from_db_err)
}

pub async fn record_package_download(
    context: &WriteContext,
    input: PackageDownloadInput,
) -> Result<package_download::Model, OrmError> {
    validate_download(&input)?;
    require_package(context, input.package_id).await?;
    require_version_belongs_to_package(context, input.package_id, input.package_version_id).await?;

    package_download::ActiveModel {
        id: Set(Uuid::new_v4()),
        package_id: Set(input.package_id),
        package_version_id: Set(input.package_version_id),
        downloaded_by_user_id: Set(input.downloaded_by_user_id),
        api_token_id: Set(input.api_token_id),
        source: Set(input.source),
        format: Set(input.format),
        bytes_sent: Set(input.bytes_sent),
        client_ip_hash: Set(input.client_ip_hash),
        user_agent: Set(input.user_agent),
        created_at: Set(chrono::Utc::now().fixed_offset()),
    }
    .insert(context.connection())
    .await
    .map_err(OrmError::from_db_err)
}

fn validate_upload(input: &PackageUploadInput) -> Result<(), OrmError> {
    required_text("requested version", &input.requested_version, 128)?;
    one_of("upload status", &input.status, UPLOAD_STATUSES)?;
    one_of(
        "upload storage backend",
        &input.storage_backend,
        STORAGE_BACKENDS,
    )?;
    optional_text("upload storage key", input.storage_key.as_deref(), 1_024)?;
    optional_one_of("upload format", input.format.as_deref(), ARCHIVE_FORMATS)?;
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
        _ => unreachable!("status was validated above"),
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

fn validate_download(input: &PackageDownloadInput) -> Result<(), OrmError> {
    one_of("download source", &input.source, DOWNLOAD_SOURCES)?;
    optional_one_of("download format", input.format.as_deref(), ARCHIVE_FORMATS)?;
    optional_nonnegative("download bytes", input.bytes_sent)?;
    optional_sha256("client IP hash", input.client_ip_hash.as_deref())?;
    optional_text("user agent", input.user_agent.as_deref(), 512)
}

async fn require_package(context: &WriteContext, package_id: Uuid) -> Result<(), OrmError> {
    let exists = package::Entity::find_by_id(package_id)
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(OrmError::not_found("package"))
    }
}

async fn require_version_belongs_to_package(
    context: &WriteContext,
    package_id: Uuid,
    package_version_id: Option<Uuid>,
) -> Result<(), OrmError> {
    let Some(package_version_id) = package_version_id else {
        return Ok(());
    };
    let version = package_version::Entity::find_by_id(package_version_id)
        .one(context.connection())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn upload(status: &str) -> PackageUploadInput {
        PackageUploadInput {
            package_id: Uuid::nil(),
            package_version_id: None,
            requested_version: "1.0.0".to_owned(),
            status: status.to_owned(),
            storage_backend: "r2".to_owned(),
            storage_key: None,
            format: None,
            size_bytes: None,
            sha256: None,
            uploaded_by_user_id: None,
            api_token_id: None,
            client_ip_hash: None,
            user_agent: None,
            error: None,
            completed_at: None,
        }
    }

    #[test]
    fn verified_uploads_require_complete_immutable_evidence() {
        let mut input = upload("verified");
        assert!(validate_upload(&input).is_err());
        input.package_version_id = Some(Uuid::nil());
        input.storage_key = Some("zed/v1/package.tar.gz".to_owned());
        input.format = Some("tar.gz".to_owned());
        input.size_bytes = Some(12);
        input.sha256 = Some("a".repeat(64));
        input.completed_at = Some(chrono::Utc::now().fixed_offset());
        assert!(validate_upload(&input).is_ok());
    }

    #[test]
    fn failed_uploads_cannot_claim_a_published_version() {
        let mut input = upload("failed");
        input.completed_at = Some(chrono::Utc::now().fixed_offset());
        assert!(validate_upload(&input).is_ok());
        input.package_version_id = Some(Uuid::nil());
        assert!(validate_upload(&input).is_err());
    }

    #[test]
    fn downloads_carry_only_hashes_not_raw_network_addresses() {
        let input = PackageDownloadInput {
            package_id: Uuid::nil(),
            package_version_id: None,
            downloaded_by_user_id: None,
            api_token_id: None,
            source: "cli".to_owned(),
            format: Some("tar.zst".to_owned()),
            bytes_sent: Some(0),
            client_ip_hash: Some("b".repeat(64)),
            user_agent: None,
        };
        assert!(validate_download(&input).is_ok());
    }
}
