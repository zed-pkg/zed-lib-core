//! One-time organization and project invitation acceptance.
//!
//! Invitation tokens are never persisted in plaintext. The caller presents a
//! bounded URL-safe token, PostgreSQL derives its SHA-256 digest, and acceptance
//! succeeds only when the verified Shared Auth email matches exactly one live
//! invitation. Membership creation and invitation consumption commit in one
//! transaction.

use sea_orm::{
    ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Statement,
    TransactionTrait, Value,
    prelude::{DateTimeWithTimeZone, Uuid},
    sea_query::OnConflict,
};

use crate::{
    OrmError, WriteContext,
    entities::{
        org, org_invitation, org_member, project, project_invitation, project_member,
    },
    models::{InvitationAcceptance, InvitationTarget, SessionIdentity},
};

/// Accept exactly one unexpired, unrevoked invitation for a verified Shared
/// Auth principal.
///
/// User projection happens before the invitation transaction because the
/// `(realm, subject)` user is independently durable. Invitation consumption and
/// membership creation are atomic. Concurrent replays race on the conditional
/// update; only one can affect a row.
pub async fn accept(
    context: &WriteContext,
    identity: &SessionIdentity,
    raw_token: &str,
) -> Result<InvitationAcceptance, OrmError> {
    validate_token(raw_token)?;
    let verified_email = normalized_identity_email(identity)?;
    let user = crate::write::upsert_user_from_session(context, identity).await?;
    let token_hash = hash_token(context, raw_token).await?;
    let accepted_at = chrono::Utc::now().fixed_offset();
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;

    let org_candidate = org_invitation::Entity::find()
        .filter(org_invitation::Column::TokenHash.eq(&token_hash))
        .filter(org_invitation::Column::AcceptedAt.is_null())
        .filter(org_invitation::Column::RevokedAt.is_null())
        .filter(org_invitation::Column::ExpiresAt.gt(accepted_at))
        .one(&transaction)
        .await
        .map_err(OrmError::from_db_err)?;
    let project_candidate = project_invitation::Entity::find()
        .filter(project_invitation::Column::TokenHash.eq(&token_hash))
        .filter(project_invitation::Column::AcceptedAt.is_null())
        .filter(project_invitation::Column::RevokedAt.is_null())
        .filter(project_invitation::Column::ExpiresAt.gt(accepted_at))
        .one(&transaction)
        .await
        .map_err(OrmError::from_db_err)?;

    let acceptance = match (org_candidate, project_candidate) {
        (Some(invitation), None) => {
            require_matching_email(&invitation.email, &verified_email)?;
            consume_org_invitation(
                &transaction,
                invitation,
                user.id,
                accepted_at,
            )
            .await?
        }
        (None, Some(invitation)) => {
            require_matching_email(&invitation.email, &verified_email)?;
            consume_project_invitation(
                &transaction,
                invitation,
                user.id,
                accepted_at,
            )
            .await?
        }
        (None, None) | (Some(_), Some(_)) => return Err(invalid_invitation()),
    };

    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(acceptance)
}

async fn consume_org_invitation<C>(
    connection: &C,
    invitation: org_invitation::Model,
    user_id: Uuid,
    accepted_at: DateTimeWithTimeZone,
) -> Result<InvitationAcceptance, OrmError>
where
    C: ConnectionTrait,
{
    consume_once(
        connection,
        "zed_org_invitations",
        invitation.id,
        user_id,
        accepted_at,
    )
    .await?;

    org_member::Entity::insert(org_member::ActiveModel {
        org_id: Set(invitation.org_id),
        user_id: Set(user_id),
        role: Set(invitation.role.clone()),
        created_at: Set(accepted_at),
        updated_at: Set(accepted_at),
    })
    .on_conflict(
        OnConflict::columns([org_member::Column::OrgId, org_member::Column::UserId])
            .do_nothing()
            .to_owned(),
    )
    .exec(connection)
    .await
    .map_err(OrmError::from_db_err)?;

    let organization = org::Entity::find_by_id(invitation.org_id)
        .one(connection)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(invalid_invitation)?;

    Ok(InvitationAcceptance {
        invitation_id: invitation.id,
        user_id,
        role: invitation.role,
        target: InvitationTarget::Organization {
            org_id: organization.id,
            org_slug: organization.slug,
        },
    })
}

async fn consume_project_invitation<C>(
    connection: &C,
    invitation: project_invitation::Model,
    user_id: Uuid,
    accepted_at: DateTimeWithTimeZone,
) -> Result<InvitationAcceptance, OrmError>
where
    C: ConnectionTrait,
{
    consume_once(
        connection,
        "zed_project_invitations",
        invitation.id,
        user_id,
        accepted_at,
    )
    .await?;

    project_member::Entity::insert(project_member::ActiveModel {
        project_id: Set(invitation.project_id),
        user_id: Set(user_id),
        role: Set(invitation.role.clone()),
        created_at: Set(accepted_at),
        updated_at: Set(accepted_at),
    })
    .on_conflict(
        OnConflict::columns([
            project_member::Column::ProjectId,
            project_member::Column::UserId,
        ])
        .do_nothing()
        .to_owned(),
    )
    .exec(connection)
    .await
    .map_err(OrmError::from_db_err)?;

    let project = project::Entity::find_by_id(invitation.project_id)
        .one(connection)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(invalid_invitation)?;
    let organization = org::Entity::find_by_id(project.org_id)
        .one(connection)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(invalid_invitation)?;

    Ok(InvitationAcceptance {
        invitation_id: invitation.id,
        user_id,
        role: invitation.role,
        target: InvitationTarget::Project {
            org_id: organization.id,
            org_slug: organization.slug,
            project_id: project.id,
            project_slug: project.slug,
        },
    })
}

async fn consume_once<C>(
    connection: &C,
    table: &'static str,
    invitation_id: Uuid,
    user_id: Uuid,
    accepted_at: DateTimeWithTimeZone,
) -> Result<(), OrmError>
where
    C: ConnectionTrait,
{
    let statement = Statement::from_sql_and_values(
        connection.get_database_backend(),
        format!(
            "UPDATE {table} \
             SET accepted_at = $1, accepted_by_user_id = $2 \
             WHERE id = $3 \
               AND accepted_at IS NULL \
               AND revoked_at IS NULL \
               AND expires_at > $1"
        ),
        [
            Value::ChronoDateTimeWithTimeZone(Some(Box::new(accepted_at))),
            Value::Uuid(Some(Box::new(user_id))),
            Value::Uuid(Some(Box::new(invitation_id))),
        ],
    );
    let result = connection
        .execute(statement)
        .await
        .map_err(OrmError::from_db_err)?;
    if result.rows_affected() == 1 {
        Ok(())
    } else {
        Err(invalid_invitation())
    }
}

async fn hash_token(context: &WriteContext, raw_token: &str) -> Result<String, OrmError> {
    let statement = Statement::from_sql_and_values(
        context.connection().get_database_backend(),
        "SELECT encode(digest($1, 'sha256'), 'hex') AS token_hash",
        [Value::String(Some(Box::new(raw_token.to_owned())))],
    );
    context
        .connection()
        .query_one(statement)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(invalid_invitation)?
        .try_get("", "token_hash")
        .map_err(OrmError::from_db_err)
}

fn normalized_identity_email(identity: &SessionIdentity) -> Result<String, OrmError> {
    identity
        .email
        .as_deref()
        .map(str::trim)
        .filter(|email| !email.is_empty())
        .map(str::to_lowercase)
        .ok_or_else(invalid_invitation)
}

fn require_matching_email(invited_email: &str, verified_email: &str) -> Result<(), OrmError> {
    if invited_email.trim().eq_ignore_ascii_case(verified_email) {
        Ok(())
    } else {
        Err(invalid_invitation())
    }
}

fn validate_token(token: &str) -> Result<(), OrmError> {
    if (32..=256).contains(&token.len())
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Ok(())
    } else {
        Err(invalid_invitation())
    }
}

fn invalid_invitation() -> OrmError {
    OrmError::not_found(
        "invitation is invalid, expired, revoked, already used, ambiguous, or belongs to another email",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(email: Option<&str>) -> SessionIdentity {
        SessionIdentity {
            subject: Uuid::nil(),
            realm: "customer".to_owned(),
            email: email.map(ToOwned::to_owned),
            display_name: None,
            avatar_url: None,
        }
    }

    #[test]
    fn token_shape_is_bounded_and_url_safe() {
        assert!(validate_token(&"a".repeat(64)).is_ok());
        assert!(validate_token(&"a".repeat(31)).is_err());
        assert!(validate_token(&"a".repeat(257)).is_err());
        assert!(validate_token(&format!("{}+", "a".repeat(63))).is_err());
    }

    #[test]
    fn invitation_email_matches_case_insensitively() {
        assert!(require_matching_email("User@Example.COM", "user@example.com").is_ok());
        assert!(require_matching_email("other@example.com", "user@example.com").is_err());
    }

    #[test]
    fn verified_email_is_required_and_normalized() {
        assert!(normalized_identity_email(&identity(None)).is_err());
        assert_eq!(
            normalized_identity_email(&identity(Some(" User@Example.COM "))).unwrap(),
            "user@example.com"
        );
    }

    #[test]
    fn invalid_cases_share_one_non_enumerating_error() {
        let token_error = validate_token("short").unwrap_err();
        let email_error = require_matching_email("a@example.com", "b@example.com").unwrap_err();
        assert_eq!(token_error, email_error);
        assert!(token_error.is_client_error());
    }
}
