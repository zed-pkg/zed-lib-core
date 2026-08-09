use sea_orm::{
    prelude::Uuid, sea_query::Expr, ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait,
    QueryFilter, TransactionTrait,
};

use crate::{
    entities::{package, package_license, package_version},
    OrmError, WriteContext,
};

use super::validation::{one_of, optional_text};

const LICENSE_KINDS: &[&str] = &["spdx", "custom", "proprietary"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageLicenseInput {
    pub package_id: Uuid,
    pub package_version_id: Option<Uuid>,
    pub kind: String,
    pub spdx_id: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
    pub text_body: Option<String>,
    pub is_primary: bool,
}

/// Add a package-level or version-specific license.
///
/// When the new row is primary, an existing primary in the same scope is
/// demoted inside the same transaction before insertion. The shared schema's
/// partial unique indexes remain the final race-safe control.
pub async fn add_package_license(
    context: &WriteContext,
    input: PackageLicenseInput,
) -> Result<package_license::Model, OrmError> {
    validate_license(&input)?;
    require_package(context, input.package_id).await?;
    require_version_belongs_to_package(context, input.package_id, input.package_version_id).await?;

    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;

    if input.is_primary {
        let mut demote = package_license::Entity::update_many()
            .col_expr(package_license::Column::IsPrimary, Expr::value(false))
            .filter(package_license::Column::PackageId.eq(input.package_id))
            .filter(package_license::Column::IsPrimary.eq(true));
        demote = match input.package_version_id {
            Some(package_version_id) => {
                demote.filter(package_license::Column::PackageVersionId.eq(package_version_id))
            }
            None => demote.filter(package_license::Column::PackageVersionId.is_null()),
        };
        demote
            .exec(&transaction)
            .await
            .map_err(OrmError::from_db_err)?;
    }

    let now = chrono::Utc::now().fixed_offset();
    let created = package_license::ActiveModel {
        id: Set(Uuid::new_v4()),
        package_id: Set(input.package_id),
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
    Ok(created)
}

fn validate_license(input: &PackageLicenseInput) -> Result<(), OrmError> {
    one_of("license kind", &input.kind, LICENSE_KINDS)?;
    optional_text("license name", input.name.as_deref(), 200)?;
    optional_text("license URL", input.url.as_deref(), 2_048)?;
    optional_text("license text", input.text_body.as_deref(), 262_144)?;

    match input.kind.as_str() {
        "spdx" => {
            let spdx = input
                .spdx_id
                .as_deref()
                .ok_or_else(|| OrmError::policy("SPDX licenses require an identifier"))?;
            if spdx.is_empty()
                || spdx.len() > 120
                || !spdx.bytes().all(|byte| {
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

async fn require_package(context: &WriteContext, package_id: Uuid) -> Result<(), OrmError> {
    if package::Entity::find_by_id(package_id)
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)?
        .is_some()
    {
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

    fn license(kind: &str) -> PackageLicenseInput {
        PackageLicenseInput {
            package_id: Uuid::nil(),
            package_version_id: None,
            kind: kind.to_owned(),
            spdx_id: None,
            name: None,
            url: None,
            text_body: None,
            is_primary: true,
        }
    }

    #[test]
    fn spdx_requires_a_schema_compatible_identifier() {
        let mut input = license("spdx");
        assert!(validate_license(&input).is_err());
        input.spdx_id = Some("Apache-2.0".to_owned());
        assert!(validate_license(&input).is_ok());
        input.spdx_id = Some("contains spaces".to_owned());
        assert!(validate_license(&input).is_err());
    }

    #[test]
    fn custom_requires_content_and_proprietary_needs_no_fake_spdx_id() {
        let mut custom = license("custom");
        assert!(validate_license(&custom).is_err());
        custom.text_body = Some("custom grant".to_owned());
        assert!(validate_license(&custom).is_ok());

        let proprietary = license("proprietary");
        assert!(validate_license(&proprietary).is_ok());
    }
}
