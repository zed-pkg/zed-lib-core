//! Mutations for package favorites and email notification preferences.

use uuid::Uuid;

use sea_orm::{ConnectionTrait, Statement};

use crate::{connection::WriteContext, error::OrmError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationPreferences {
    pub email_enabled: bool,
    pub favorite_package_email: bool,
    pub downloaded_package_email: bool,
    pub major_release_email: bool,
    pub security_email: bool,
    pub digest_email: bool,
    pub digest_frequency: String,
    pub minimum_security_severity: String,
    pub timezone: String,
    pub digest_hour: i16,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            email_enabled: true,
            favorite_package_email: true,
            downloaded_package_email: true,
            major_release_email: true,
            security_email: true,
            digest_email: true,
            digest_frequency: "weekly".to_owned(),
            minimum_security_severity: "high".to_owned(),
            timezone: "UTC".to_owned(),
            digest_hour: 9,
        }
    }
}

pub async fn favorite_package(
    context: &WriteContext,
    user_id: Uuid,
    package_id: Uuid,
) -> Result<bool, OrmError> {
    let result = context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "insert into zed_package_favorites(user_id, package_id) values ($1, $2) on conflict do nothing",
            [user_id.into(), package_id.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(result.rows_affected() == 1)
}

pub async fn unfavorite_package(
    context: &WriteContext,
    user_id: Uuid,
    package_id: Uuid,
) -> Result<bool, OrmError> {
    let result = context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "delete from zed_package_favorites where user_id = $1 and package_id = $2",
            [user_id.into(), package_id.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(result.rows_affected() == 1)
}

pub async fn is_package_favorite(
    context: &WriteContext,
    user_id: Uuid,
    package_id: Uuid,
) -> Result<bool, OrmError> {
    Ok(context
        .connection()
        .query_one(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "select 1 from zed_package_favorites where user_id = $1 and package_id = $2",
            [user_id.into(), package_id.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?
        .is_some())
}

pub async fn notification_preferences(
    context: &WriteContext,
    user_id: Uuid,
) -> Result<NotificationPreferences, OrmError> {
    let Some(row) = context
        .connection()
        .query_one(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            r#"
            select email_enabled, favorite_package_email, downloaded_package_email,
                   major_release_email, security_email, digest_email,
                   digest_frequency, minimum_security_severity, timezone, digest_hour
              from zed_notification_preferences
             where user_id = $1
            "#,
            [user_id.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?
    else {
        return Ok(NotificationPreferences::default());
    };

    Ok(NotificationPreferences {
        email_enabled: row
            .try_get("", "email_enabled")
            .map_err(OrmError::from_db_err)?,
        favorite_package_email: row
            .try_get("", "favorite_package_email")
            .map_err(OrmError::from_db_err)?,
        downloaded_package_email: row
            .try_get("", "downloaded_package_email")
            .map_err(OrmError::from_db_err)?,
        major_release_email: row
            .try_get("", "major_release_email")
            .map_err(OrmError::from_db_err)?,
        security_email: row
            .try_get("", "security_email")
            .map_err(OrmError::from_db_err)?,
        digest_email: row
            .try_get("", "digest_email")
            .map_err(OrmError::from_db_err)?,
        digest_frequency: row
            .try_get("", "digest_frequency")
            .map_err(OrmError::from_db_err)?,
        minimum_security_severity: row
            .try_get("", "minimum_security_severity")
            .map_err(OrmError::from_db_err)?,
        timezone: row
            .try_get("", "timezone")
            .map_err(OrmError::from_db_err)?,
        digest_hour: row
            .try_get("", "digest_hour")
            .map_err(OrmError::from_db_err)?,
    })
}

pub async fn upsert_notification_preferences(
    context: &WriteContext,
    user_id: Uuid,
    preferences: &NotificationPreferences,
) -> Result<(), OrmError> {
    validate_preferences(preferences)?;
    context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            r#"
            insert into zed_notification_preferences (
              user_id, email_enabled, favorite_package_email, downloaded_package_email,
              major_release_email, security_email, digest_email, digest_frequency,
              minimum_security_severity, timezone, digest_hour, unsubscribed_at
            ) values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,null)
            on conflict (user_id) do update set
              email_enabled = excluded.email_enabled,
              favorite_package_email = excluded.favorite_package_email,
              downloaded_package_email = excluded.downloaded_package_email,
              major_release_email = excluded.major_release_email,
              security_email = excluded.security_email,
              digest_email = excluded.digest_email,
              digest_frequency = excluded.digest_frequency,
              minimum_security_severity = excluded.minimum_security_severity,
              timezone = excluded.timezone,
              digest_hour = excluded.digest_hour,
              unsubscribed_at = case when excluded.email_enabled then null else zed_notification_preferences.unsubscribed_at end
            "#,
            [
                user_id.into(),
                preferences.email_enabled.into(),
                preferences.favorite_package_email.into(),
                preferences.downloaded_package_email.into(),
                preferences.major_release_email.into(),
                preferences.security_email.into(),
                preferences.digest_email.into(),
                preferences.digest_frequency.clone().into(),
                preferences.minimum_security_severity.clone().into(),
                preferences.timezone.clone().into(),
                preferences.digest_hour.into(),
            ],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(())
}

/// One-click unsubscribe is deliberately coarse: it disables all product email
/// while leaving account/authentication email under Supabase/Shared Auth.
pub async fn unsubscribe_product_email(
    context: &WriteContext,
    user_id: Uuid,
) -> Result<(), OrmError> {
    context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            r#"
            insert into zed_notification_preferences(user_id, email_enabled, unsubscribed_at)
            values ($1, false, now())
            on conflict (user_id) do update set
              email_enabled = false,
              unsubscribed_at = now()
            "#,
            [user_id.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(())
}

fn validate_preferences(preferences: &NotificationPreferences) -> Result<(), OrmError> {
    if !matches!(preferences.digest_frequency.as_str(), "daily" | "weekly" | "never") {
        return Err(OrmError::policy(
            "digest_frequency must be daily, weekly, or never",
        ));
    }
    if !matches!(
        preferences.minimum_security_severity.as_str(),
        "moderate" | "high" | "critical"
    ) {
        return Err(OrmError::policy(
            "minimum_security_severity must be moderate, high, or critical",
        ));
    }
    if !(0..=23).contains(&preferences.digest_hour) {
        return Err(OrmError::policy("digest_hour must be between 0 and 23"));
    }
    if preferences.timezone.trim().is_empty() || preferences.timezone.len() > 96 {
        return Err(OrmError::policy("timezone must contain 1 to 96 bytes"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe_and_useful() {
        let defaults = NotificationPreferences::default();
        assert!(defaults.email_enabled);
        assert!(defaults.security_email);
        assert!(defaults.major_release_email);
        assert_eq!(defaults.digest_frequency, "weekly");
        assert_eq!(defaults.minimum_security_severity, "high");
    }

    #[test]
    fn invalid_preference_values_fail_before_database_io() {
        let mut preferences = NotificationPreferences::default();
        preferences.digest_frequency = "hourly".into();
        assert!(validate_preferences(&preferences).is_err());
        preferences.digest_frequency = "daily".into();
        preferences.digest_hour = 24;
        assert!(validate_preferences(&preferences).is_err());
    }
}
