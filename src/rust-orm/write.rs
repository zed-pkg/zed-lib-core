//! Write surface — API servers only (`read-write` feature).
//!
//! Every symbol in this module disappears from default builds. Add only named
//! mutations that preserve API-owned authorization, validation, invariants,
//! audit behavior, and transaction boundaries; never expose a raw ORM session.
//! The feature gate expresses intent; the authoritative control is the
//! database principal, whose grants deny web identities all DML.

use crate::{connection::inspect_connection, read::ConnectionState, OrmError, WriteContext};

/// Return safe policy evidence for the API's opaque write context.
pub async fn connection_state(context: &WriteContext) -> Result<ConnectionState, OrmError> {
    inspect_connection(context.connection())
        .await
        .map(ConnectionState::from_internal)
}

/// Lightweight readiness check for an API consumer.
pub async fn ping(context: &WriteContext) -> Result<(), OrmError> {
    let state = connection_state(context).await?;
    if state.transaction_read_only() {
        Err(OrmError::policy(
            "write context unexpectedly became transaction read-only",
        ))
    } else {
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Registry writes.
//
// Named mutations only. Each one owns its validation and, where the schema has
// an opinion, defers to it rather than re-implementing it — most importantly
// the private→public promotion rule, which is pre-checked here for a clean
// error and enforced in the database regardless.
// ─────────────────────────────────────────────────────────────────────────────

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use crate::entities::{
    org, org_invitation, org_member, package, package_download, project, project_member, user,
};
use crate::models::{InvitationReceipt, SessionIdentity, UserSettingsInput, UserSummary};
use crate::policy::PromotionRefusal;
use crate::read::user_summary;

/// Map a Shared Auth session onto its registry user, creating it on first sight.
///
/// This is the only place the two data planes are reconciled. It is keyed on
/// `(realm, subject)` — the unique index backing it — so a concurrent first
/// login cannot create two rows for one principal.
pub async fn upsert_user_from_session(
    context: &WriteContext,
    identity: &SessionIdentity,
) -> Result<UserSummary, OrmError> {
    let existing = user::Entity::find()
        .filter(user::Column::AuthRealm.eq(identity.realm.as_str()))
        .filter(user::Column::SharedAuthSubject.eq(identity.subject))
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;

    if let Some(found) = existing {
        // Refresh the mirrored profile fields; the IdP is authoritative for them.
        let mut active: user::ActiveModel = found.into();
        active.email = Set(identity.email.clone());
        active.display_name = Set(identity.display_name.clone());
        active.avatar_url = Set(identity.avatar_url.clone());
        let updated = active
            .update(context.connection())
            .await
            .map_err(OrmError::from_db_err)?;
        return Ok(user_summary(updated));
    }

    let created = user::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        shared_auth_subject: Set(identity.subject),
        auth_realm: Set(identity.realm.clone()),
        email: Set(identity.email.clone()),
        display_name: Set(identity.display_name.clone()),
        avatar_url: Set(identity.avatar_url.clone()),
        settings: Set(serde_json::Value::Object(Default::default())),
        is_soft_deleted: Set(false),
        ..Default::default()
    }
    .insert(context.connection())
    .await
    .map_err(OrmError::from_db_err)?;

    Ok(user_summary(created))
}

pub async fn update_user_settings(
    context: &WriteContext,
    user_id: uuid::Uuid,
    input: &UserSettingsInput,
) -> Result<UserSummary, OrmError> {
    let found = user::Entity::find_by_id(user_id)
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| OrmError::not_found("user"))?;

    let mut active: user::ActiveModel = found.into();
    active.display_name = Set(input.display_name.clone());
    active.avatar_url = Set(input.avatar_url.clone());
    active.settings = Set(input.settings.clone());
    let updated = active
        .update(context.connection())
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(user_summary(updated))
}

/// Create an org and make its creator the owner, atomically.
///
/// The two writes are one transaction so an org can never exist with nobody
/// able to administer it.
pub async fn create_org(
    context: &WriteContext,
    creator_user_id: uuid::Uuid,
    slug: &str,
    name: &str,
    description: Option<&str>,
) -> Result<org::Model, OrmError> {
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;

    let created = org::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        slug: Set(slug.to_owned()),
        name: Set(name.to_owned()),
        description: Set(description.map(ToOwned::to_owned)),
        settings: Set(serde_json::Value::Object(Default::default())),
        created_by_user_id: Set(Some(creator_user_id)),
        is_soft_deleted: Set(false),
        ..Default::default()
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;

    org_member::ActiveModel {
        org_id: Set(created.id),
        user_id: Set(creator_user_id),
        role: Set("owner".to_owned()),
        ..Default::default()
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;

    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(created)
}

pub async fn create_project(
    context: &WriteContext,
    org_id: uuid::Uuid,
    creator_user_id: uuid::Uuid,
    slug: &str,
    name: &str,
    description: Option<&str>,
) -> Result<project::Model, OrmError> {
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;

    let created = project::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        org_id: Set(org_id),
        slug: Set(slug.to_owned()),
        name: Set(name.to_owned()),
        description: Set(description.map(ToOwned::to_owned)),
        visibility: Set("private".to_owned()),
        settings: Set(serde_json::Value::Object(Default::default())),
        created_by_user_id: Set(Some(creator_user_id)),
        is_soft_deleted: Set(false),
        ..Default::default()
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;

    project_member::ActiveModel {
        project_id: Set(created.id),
        user_id: Set(creator_user_id),
        role: Set("owner".to_owned()),
        ..Default::default()
    }
    .insert(&transaction)
    .await
    .map_err(OrmError::from_db_err)?;

    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(created)
}

/// Create a package. New packages start private; going public is a separate,
/// policy-checked transition.
pub async fn create_package(
    context: &WriteContext,
    org_id: uuid::Uuid,
    project_id: Option<uuid::Uuid>,
    creator_user_id: uuid::Uuid,
    name: &str,
    description: Option<&str>,
) -> Result<package::Model, OrmError> {
    package::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        org_id: Set(org_id),
        project_id: Set(project_id),
        name: Set(name.to_owned()),
        description: Set(description.map(ToOwned::to_owned)),
        visibility: Set("private".to_owned()),
        vcs: Set("git".to_owned()),
        repo_url: Set(String::new()),
        keywords: Set(serde_json::Value::Array(Vec::new())),
        config: Set(serde_json::Value::Object(Default::default())),
        created_by_user_id: Set(Some(creator_user_id)),
        is_soft_deleted: Set(false),
        ..Default::default()
    }
    .insert(context.connection())
    .await
    .map_err(OrmError::from_db_err)
}

/// Change a package's visibility, honouring the promotion window and the
/// permanent-public cache contract.
///
/// The window is checked here first so the caller gets a typed refusal with a
/// usable message. That check is a courtesy, not the control: the
/// `zed_packages_visibility_guard` trigger re-evaluates it inside the same
/// statement, and a promotion that races past the pre-check still fails with
/// `ZD001`/`ZD002`, which [`OrmError::from_db_err`] maps to the same variants.
pub async fn set_package_visibility(
    context: &WriteContext,
    package_id: uuid::Uuid,
    visibility: &str,
) -> Result<package::Model, OrmError> {
    let found = package::Entity::find_by_id(package_id)
        .one(context.connection())
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| OrmError::not_found("package"))?;

    if found.visibility == "public" && visibility != "public" {
        return Err(OrmError::PublicVisibilityIsPermanent(
            "a public package cannot become private or internal".to_owned(),
        ));
    }

    if visibility == "public" && found.visibility != "public" {
        let limits = crate::policy::VisibilityLimits::load(context.connection()).await?;
        let age_days = age_in_days(&found.created_at);
        if let Some(refusal) = limits.evaluate(age_days, found.download_count) {
            return Err(match refusal {
                PromotionRefusal::TooOld { .. } => {
                    OrmError::VisibilityWindowExpired(refusal.to_string())
                }
                PromotionRefusal::TooManyDownloads { .. } => {
                    OrmError::VisibilityDownloadLimitExceeded(refusal.to_string())
                }
            });
        }
    }

    let mut active: package::ActiveModel = found.into();
    active.visibility = Set(visibility.to_owned());
    active
        .update(context.connection())
        .await
        .map_err(OrmError::from_db_err)
}

/// Whole days elapsed since `created_at`, matching the trigger's arithmetic
/// (`extract(epoch from now() - created_at) / 86400`).
fn age_in_days(created_at: &sea_orm::prelude::DateTimeWithTimeZone) -> f64 {
    let elapsed = chrono::Utc::now().fixed_offset() - *created_at;
    elapsed.num_seconds() as f64 / 86_400.0
}

/// Record a download. The package and version counters are maintained by a
/// database trigger, so this inserts the ledger row and nothing else — bumping
/// a counter here as well would double-count and corrupt the promotion rule.
pub async fn record_download(
    context: &WriteContext,
    package_id: uuid::Uuid,
    package_version_id: Option<uuid::Uuid>,
    downloaded_by_user_id: Option<uuid::Uuid>,
    source: &str,
    bytes_sent: Option<i64>,
) -> Result<(), OrmError> {
    package_download::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        package_id: Set(package_id),
        package_version_id: Set(package_version_id),
        downloaded_by_user_id: Set(downloaded_by_user_id),
        source: Set(source.to_owned()),
        bytes_sent: Set(bytes_sent),
        ..Default::default()
    }
    .insert(context.connection())
    .await
    .map_err(OrmError::from_db_err)?;
    Ok(())
}

/// Complete, validated-by-the-caller command for creating one organization
/// invitation.
///
/// The one-time token and its digest deliberately travel together through a
/// single typed boundary. The persistence layer stores only `token_hash`; the
/// plaintext token exists solely so it can be returned once in the receipt.
#[derive(Debug, Clone)]
pub struct OrgInvitationInput<'a> {
    pub org_id: uuid::Uuid,
    pub invited_by_user_id: uuid::Uuid,
    pub email: &'a str,
    pub role: &'a str,
    pub token: &'a str,
    pub token_hash: &'a str,
    pub expires_at: sea_orm::prelude::DateTimeWithTimeZone,
}

/// Invite someone to an org. The caller receives the one-time token; only its
/// SHA-256 digest is stored, so a database read cannot be replayed as an invite.
pub async fn invite_to_org(
    context: &WriteContext,
    invitation: OrgInvitationInput<'_>,
) -> Result<InvitationReceipt, OrmError> {
    let OrgInvitationInput {
        org_id,
        invited_by_user_id,
        email,
        role,
        token,
        token_hash,
        expires_at,
    } = invitation;

    let created = org_invitation::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        org_id: Set(org_id),
        invited_by_user_id: Set(invited_by_user_id),
        email: Set(email.to_owned()),
        role: Set(role.to_owned()),
        token_hash: Set(token_hash.to_owned()),
        expires_at: Set(expires_at),
        ..Default::default()
    }
    .insert(context.connection())
    .await
    .map_err(OrmError::from_db_err)?;

    Ok(InvitationReceipt {
        invitation_id: created.id,
        token: token.to_owned(),
        email: created.email,
        role: created.role,
    })
}
