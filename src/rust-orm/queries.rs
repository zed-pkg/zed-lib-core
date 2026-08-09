//! Named registry operations.
//!
//! Services call these functions rather than exposing raw SeaORM sessions,
//! entities, or query builders across repository boundaries.

pub mod read {
    use std::collections::HashMap;

    use sea_orm::{
        ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    };

    use crate::entities::{org, org_member, package, project, project_member, user};
    use crate::models::{
        HomePageData, OrgDashboardData, OrgSummary, PackageSummary, ProjectSummary, UserSummary,
    };

    const RESULT_LIMIT: u64 = 200;

    pub async fn healthcheck(conn: &DatabaseConnection) -> Result<(), DbErr> {
        use sea_orm::{ConnectionTrait, Statement};
        let statement = Statement::from_string(conn.get_database_backend(), "SELECT 1");
        conn.query_one(statement).await.map(|_| ())
    }

    pub async fn user_by_subject(
        conn: &DatabaseConnection,
        subject: &str,
    ) -> Result<Option<UserSummary>, DbErr> {
        user::Entity::find()
            .filter(user::Column::SharedAuthSubject.eq(subject))
            .one(conn)
            .await
            .map(|model| model.map(user_summary))
    }

    pub async fn home_for_user(
        conn: &DatabaseConnection,
        subject: &str,
        query: &str,
    ) -> Result<HomePageData, DbErr> {
        let normalized_query = normalize_query(query);
        let Some(user_model) = user::Entity::find()
            .filter(user::Column::SharedAuthSubject.eq(subject))
            .one(conn)
            .await?
        else {
            return Ok(HomePageData {
                user: None,
                orgs: Vec::new(),
                projects: Vec::new(),
                packages: Vec::new(),
                query: normalized_query,
            });
        };

        let memberships = org_member::Entity::find()
            .filter(org_member::Column::UserId.eq(user_model.id))
            .limit(RESULT_LIMIT)
            .all(conn)
            .await?;
        let org_roles = memberships
            .iter()
            .map(|membership| (membership.org_id, membership.role.clone()))
            .collect::<HashMap<_, _>>();
        let org_ids = org_roles.keys().copied().collect::<Vec<_>>();

        if org_ids.is_empty() {
            return Ok(HomePageData {
                user: Some(user_summary(user_model)),
                orgs: Vec::new(),
                projects: Vec::new(),
                packages: Vec::new(),
                query: normalized_query,
            });
        }

        let org_models = org::Entity::find()
            .filter(org::Column::Id.is_in(org_ids.clone()))
            .order_by_asc(org::Column::Name)
            .limit(RESULT_LIMIT)
            .all(conn)
            .await?;
        let org_slugs = org_models
            .iter()
            .map(|model| (model.id, model.slug.clone()))
            .collect::<HashMap<_, _>>();
        let orgs = org_models
            .into_iter()
            .map(|model| OrgSummary {
                role: org_roles
                    .get(&model.id)
                    .cloned()
                    .unwrap_or_else(|| "reader".into()),
                id: model.id,
                slug: model.slug,
                name: model.name,
                description: model.description,
            })
            .collect::<Vec<_>>();

        let project_roles = project_member::Entity::find()
            .filter(project_member::Column::UserId.eq(user_model.id))
            .limit(RESULT_LIMIT)
            .all(conn)
            .await?
            .into_iter()
            .map(|membership| (membership.project_id, membership.role))
            .collect::<HashMap<_, _>>();

        let mut projects = project::Entity::find()
            .filter(project::Column::OrgId.is_in(org_ids.clone()))
            .order_by_asc(project::Column::Name)
            .limit(RESULT_LIMIT)
            .all(conn)
            .await?
            .into_iter()
            .filter(|model| searchable_project(model, &normalized_query))
            .map(|model| ProjectSummary {
                id: model.id,
                org_id: model.org_id,
                org_slug: org_slugs.get(&model.org_id).cloned().unwrap_or_default(),
                slug: model.slug,
                name: model.name,
                description: model.description,
                role: project_roles
                    .get(&model.id)
                    .cloned()
                    .or_else(|| org_roles.get(&model.org_id).cloned())
                    .unwrap_or_else(|| "reader".into()),
            })
            .collect::<Vec<_>>();

        let project_slugs = projects
            .iter()
            .map(|model| (model.id, model.slug.clone()))
            .collect::<HashMap<_, _>>();

        let mut packages = package::Entity::find()
            .filter(package::Column::OrgId.is_in(org_ids))
            .order_by_asc(package::Column::Name)
            .limit(RESULT_LIMIT)
            .all(conn)
            .await?
            .into_iter()
            .filter(|model| searchable_package(model, &normalized_query))
            .map(|model| PackageSummary {
                id: model.id,
                org_id: model.org_id,
                org_slug: org_slugs.get(&model.org_id).cloned().unwrap_or_default(),
                project_id: model.project_id,
                project_slug: model
                    .project_id
                    .and_then(|id| project_slugs.get(&id).cloned()),
                name: model.name,
                description: model.description,
                visibility: model.visibility,
                repo_url: model.repo_url,
                config: model.config,
            })
            .collect::<Vec<_>>();

        projects.truncate(100);
        packages.truncate(100);

        Ok(HomePageData {
            user: Some(user_summary(user_model)),
            orgs,
            projects,
            packages,
            query: normalized_query,
        })
    }

    pub async fn org_dashboard_for_user(
        conn: &DatabaseConnection,
        subject: &str,
        org_slug: &str,
    ) -> Result<Option<OrgDashboardData>, DbErr> {
        let Some(user_model) = user::Entity::find()
            .filter(user::Column::SharedAuthSubject.eq(subject))
            .one(conn)
            .await?
        else {
            return Ok(None);
        };
        let Some(org_model) = org::Entity::find()
            .filter(org::Column::Slug.eq(org_slug))
            .one(conn)
            .await?
        else {
            return Ok(None);
        };
        let Some(org_membership) = org_member::Entity::find()
            .filter(org_member::Column::OrgId.eq(org_model.id))
            .filter(org_member::Column::UserId.eq(user_model.id))
            .one(conn)
            .await?
        else {
            return Ok(None);
        };

        let project_roles = project_member::Entity::find()
            .filter(project_member::Column::UserId.eq(user_model.id))
            .all(conn)
            .await?
            .into_iter()
            .map(|membership| (membership.project_id, membership.role))
            .collect::<HashMap<_, _>>();

        let project_models = project::Entity::find()
            .filter(project::Column::OrgId.eq(org_model.id))
            .order_by_asc(project::Column::Name)
            .limit(RESULT_LIMIT)
            .all(conn)
            .await?;
        let project_slugs = project_models
            .iter()
            .map(|model| (model.id, model.slug.clone()))
            .collect::<HashMap<_, _>>();
        let projects = project_models
            .into_iter()
            .map(|model| ProjectSummary {
                id: model.id,
                org_id: model.org_id,
                org_slug: org_model.slug.clone(),
                slug: model.slug,
                name: model.name,
                description: model.description,
                role: project_roles
                    .get(&model.id)
                    .cloned()
                    .unwrap_or_else(|| org_membership.role.clone()),
            })
            .collect();

        let packages = package::Entity::find()
            .filter(package::Column::OrgId.eq(org_model.id))
            .order_by_asc(package::Column::Name)
            .limit(RESULT_LIMIT)
            .all(conn)
            .await?
            .into_iter()
            .map(|model| PackageSummary {
                id: model.id,
                org_id: model.org_id,
                org_slug: org_model.slug.clone(),
                project_id: model.project_id,
                project_slug: model
                    .project_id
                    .and_then(|id| project_slugs.get(&id).cloned()),
                name: model.name,
                description: model.description,
                visibility: model.visibility,
                repo_url: model.repo_url,
                config: model.config,
            })
            .collect();

        Ok(Some(OrgDashboardData {
            org: OrgSummary {
                id: org_model.id,
                slug: org_model.slug,
                name: org_model.name,
                description: org_model.description,
                role: org_membership.role,
            },
            projects,
            packages,
        }))
    }

    pub async fn package_for_user(
        conn: &DatabaseConnection,
        subject: &str,
        org_slug: &str,
        package_name: &str,
    ) -> Result<Option<PackageSummary>, DbErr> {
        let Some(dashboard) = org_dashboard_for_user(conn, subject, org_slug).await? else {
            return Ok(None);
        };
        Ok(dashboard
            .packages
            .into_iter()
            .find(|package| package.name == package_name))
    }

    pub async fn project_for_user(
        conn: &DatabaseConnection,
        subject: &str,
        org_slug: &str,
        project_slug: &str,
    ) -> Result<Option<ProjectSummary>, DbErr> {
        let Some(dashboard) = org_dashboard_for_user(conn, subject, org_slug).await? else {
            return Ok(None);
        };
        Ok(dashboard
            .projects
            .into_iter()
            .find(|project| project.slug == project_slug))
    }

    fn user_summary(model: user::Model) -> UserSummary {
        UserSummary {
            id: model.id,
            subject: model.shared_auth_subject,
            email: model.email,
            display_name: model.display_name,
            avatar_url: model.avatar_url,
            settings: model.settings,
        }
    }

    fn normalize_query(query: &str) -> String {
        query
            .trim()
            .chars()
            .take(100)
            .collect::<String>()
            .to_lowercase()
    }

    fn searchable_project(model: &project::Model, query: &str) -> bool {
        query.is_empty()
            || model.slug.to_lowercase().contains(query)
            || model.name.to_lowercase().contains(query)
            || model
                .description
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(query)
    }

    fn searchable_package(model: &package::Model, query: &str) -> bool {
        query.is_empty()
            || model.name.to_lowercase().contains(query)
            || model
                .description
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(query)
            || model.repo_url.to_lowercase().contains(query)
    }
}

pub mod write {
    use std::time::SystemTime;

    use sea_orm::{
        ActiveModelTrait,
        ActiveValue::Set,
        ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter,
        Statement, TransactionTrait,
        prelude::{DateTimeUtc, DateTimeWithTimeZone, Json, Uuid},
    };

    use crate::entities::{
        org, org_invitation, org_member, package, project, project_invitation, project_member, user,
    };
    use crate::models::{
        InvitationReceipt, PackageSettingsInput, SessionIdentity, UserSettingsInput, UserSummary,
    };

    struct RandomMaterial {
        id: Uuid,
        token: String,
        token_hash: String,
        expires_at: DateTimeWithTimeZone,
    }

    pub async fn ensure_user(
        conn: &DatabaseConnection,
        identity: &SessionIdentity,
    ) -> Result<UserSummary, DbErr> {
        let now = now();
        if let Some(existing) = user::Entity::find()
            .filter(user::Column::SharedAuthSubject.eq(&identity.subject))
            .one(conn)
            .await?
        {
            let mut active: user::ActiveModel = existing.into();
            active.email = Set(identity.email.clone());
            if identity.display_name.is_some() {
                active.display_name = Set(identity.display_name.clone());
            }
            if identity.avatar_url.is_some() {
                active.avatar_url = Set(identity.avatar_url.clone());
            }
            active.updated_at = Set(now);
            let model = active.update(conn).await?;
            return Ok(user_summary(model));
        }

        let model = user::ActiveModel {
            id: Set(new_uuid(conn).await?),
            shared_auth_subject: Set(identity.subject.clone()),
            email: Set(identity.email.clone()),
            display_name: Set(identity.display_name.clone()),
            avatar_url: Set(identity.avatar_url.clone()),
            settings: Set(empty_json()),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(conn)
        .await?;
        Ok(user_summary(model))
    }

    pub async fn create_org(
        conn: &DatabaseConnection,
        subject: &str,
        slug: &str,
        name: &str,
    ) -> Result<org::Model, DbErr> {
        validate_slug(slug)?;
        let user_model = require_user(conn, subject).await?;
        let now = now();
        let txn = conn.begin().await?;

        let org_model = org::ActiveModel {
            id: Set(new_uuid(&txn).await?),
            slug: Set(slug.to_owned()),
            name: Set(name.trim().to_owned()),
            description: Set(None),
            settings: Set(empty_json()),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await?;

        org_member::ActiveModel {
            org_id: Set(org_model.id),
            user_id: Set(user_model.id),
            role: Set("owner".into()),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await?;

        txn.commit().await?;
        Ok(org_model)
    }

    pub async fn create_project(
        conn: &DatabaseConnection,
        subject: &str,
        org_slug: &str,
        slug: &str,
        name: &str,
    ) -> Result<project::Model, DbErr> {
        validate_slug(slug)?;
        let (user_model, org_model, role) = require_org_member(conn, subject, org_slug).await?;
        require_admin(&role)?;
        let now = now();
        let txn = conn.begin().await?;

        let project_model = project::ActiveModel {
            id: Set(new_uuid(&txn).await?),
            org_id: Set(org_model.id),
            slug: Set(slug.to_owned()),
            name: Set(name.trim().to_owned()),
            description: Set(None),
            settings: Set(empty_json()),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await?;

        project_member::ActiveModel {
            project_id: Set(project_model.id),
            user_id: Set(user_model.id),
            role: Set("owner".into()),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await?;

        txn.commit().await?;
        Ok(project_model)
    }

    pub async fn invite_org_member(
        conn: &DatabaseConnection,
        subject: &str,
        org_slug: &str,
        email: &str,
        role: &str,
    ) -> Result<InvitationReceipt, DbErr> {
        let (user_model, org_model, caller_role) =
            require_org_member(conn, subject, org_slug).await?;
        require_admin(&caller_role)?;
        validate_invite_role(role)?;
        create_org_invitation(conn, user_model.id, org_model.id, email, role).await
    }

    pub async fn invite_project_member(
        conn: &DatabaseConnection,
        subject: &str,
        org_slug: &str,
        project_slug: &str,
        email: &str,
        role: &str,
    ) -> Result<InvitationReceipt, DbErr> {
        let (user_model, org_model, caller_role) =
            require_org_member(conn, subject, org_slug).await?;
        let project_model = project::Entity::find()
            .filter(project::Column::OrgId.eq(org_model.id))
            .filter(project::Column::Slug.eq(project_slug))
            .one(conn)
            .await?
            .ok_or_else(|| DbErr::Custom("project not found".into()))?;
        let project_role = project_member::Entity::find()
            .filter(project_member::Column::ProjectId.eq(project_model.id))
            .filter(project_member::Column::UserId.eq(user_model.id))
            .one(conn)
            .await?
            .map(|membership| membership.role)
            .unwrap_or(caller_role);
        require_admin(&project_role)?;
        validate_invite_role(role)?;
        create_project_invitation(conn, user_model.id, project_model.id, email, role).await
    }

    pub async fn update_package_settings(
        conn: &DatabaseConnection,
        subject: &str,
        org_slug: &str,
        package_name: &str,
        input: PackageSettingsInput,
    ) -> Result<package::Model, DbErr> {
        let (_, org_model, role) = require_org_member(conn, subject, org_slug).await?;
        require_admin(&role)?;
        validate_visibility(&input.visibility)?;

        if let Some(project_id) = input.project_id {
            let project_exists = project::Entity::find_by_id(project_id)
                .filter(project::Column::OrgId.eq(org_model.id))
                .one(conn)
                .await?
                .is_some();
            if !project_exists {
                return Err(DbErr::Custom(
                    "package project must belong to the same organization".into(),
                ));
            }
        }

        let existing = package::Entity::find()
            .filter(package::Column::OrgId.eq(org_model.id))
            .filter(package::Column::Name.eq(package_name))
            .one(conn)
            .await?
            .ok_or_else(|| DbErr::Custom("package not found".into()))?;
        let mut active: package::ActiveModel = existing.into();
        active.project_id = Set(input.project_id);
        active.description = Set(input.description);
        active.visibility = Set(input.visibility);
        active.config = Set(input.config);
        active.updated_at = Set(now());
        active.update(conn).await
    }

    pub async fn update_user_settings(
        conn: &DatabaseConnection,
        subject: &str,
        input: UserSettingsInput,
    ) -> Result<UserSummary, DbErr> {
        let model = require_user(conn, subject).await?;
        let mut active: user::ActiveModel = model.into();
        active.display_name = Set(input.display_name);
        active.avatar_url = Set(input.avatar_url);
        active.settings = Set(input.settings);
        active.updated_at = Set(now());
        active.update(conn).await.map(user_summary)
    }

    async fn create_org_invitation(
        conn: &DatabaseConnection,
        inviter: Uuid,
        org_id: Uuid,
        email: &str,
        role: &str,
    ) -> Result<InvitationReceipt, DbErr> {
        let material = random_material(conn).await?;
        let email = normalize_email(email)?;
        org_invitation::ActiveModel {
            id: Set(material.id),
            org_id: Set(org_id),
            invited_by_user_id: Set(inviter),
            email: Set(email.clone()),
            role: Set(role.to_owned()),
            token_hash: Set(material.token_hash),
            expires_at: Set(material.expires_at),
            accepted_at: Set(None),
            created_at: Set(now()),
        }
        .insert(conn)
        .await?;
        Ok(InvitationReceipt {
            invitation_id: material.id,
            token: material.token,
            email,
            role: role.to_owned(),
        })
    }

    async fn create_project_invitation(
        conn: &DatabaseConnection,
        inviter: Uuid,
        project_id: Uuid,
        email: &str,
        role: &str,
    ) -> Result<InvitationReceipt, DbErr> {
        let material = random_material(conn).await?;
        let email = normalize_email(email)?;
        project_invitation::ActiveModel {
            id: Set(material.id),
            project_id: Set(project_id),
            invited_by_user_id: Set(inviter),
            email: Set(email.clone()),
            role: Set(role.to_owned()),
            token_hash: Set(material.token_hash),
            expires_at: Set(material.expires_at),
            accepted_at: Set(None),
            created_at: Set(now()),
        }
        .insert(conn)
        .await?;
        Ok(InvitationReceipt {
            invitation_id: material.id,
            token: material.token,
            email,
            role: role.to_owned(),
        })
    }

    async fn require_user(conn: &DatabaseConnection, subject: &str) -> Result<user::Model, DbErr> {
        user::Entity::find()
            .filter(user::Column::SharedAuthSubject.eq(subject))
            .one(conn)
            .await?
            .ok_or_else(|| DbErr::Custom("registry user not found".into()))
    }

    async fn require_org_member(
        conn: &DatabaseConnection,
        subject: &str,
        org_slug: &str,
    ) -> Result<(user::Model, org::Model, String), DbErr> {
        let user_model = require_user(conn, subject).await?;
        let org_model = org::Entity::find()
            .filter(org::Column::Slug.eq(org_slug))
            .one(conn)
            .await?
            .ok_or_else(|| DbErr::Custom("organization not found".into()))?;
        let membership = org_member::Entity::find()
            .filter(org_member::Column::OrgId.eq(org_model.id))
            .filter(org_member::Column::UserId.eq(user_model.id))
            .one(conn)
            .await?
            .ok_or_else(|| DbErr::Custom("organization membership required".into()))?;
        Ok((user_model, org_model, membership.role))
    }

    async fn new_uuid<C>(conn: &C) -> Result<Uuid, DbErr>
    where
        C: ConnectionTrait,
    {
        let row = conn
            .query_one(Statement::from_string(
                conn.get_database_backend(),
                "SELECT gen_random_uuid() AS id",
            ))
            .await?
            .ok_or_else(|| DbErr::Custom("Postgres returned no UUID".into()))?;
        row.try_get("", "id")
    }

    async fn random_material<C>(conn: &C) -> Result<RandomMaterial, DbErr>
    where
        C: ConnectionTrait,
    {
        let row = conn
            .query_one(Statement::from_string(
                conn.get_database_backend(),
                "WITH material AS (\
                   SELECT gen_random_uuid() AS id,\
                          encode(gen_random_bytes(32), 'hex') AS token\
                 )\
                 SELECT id, token,\
                        encode(digest(token, 'sha256'), 'hex') AS token_hash,\
                        now() + interval '7 days' AS expires_at\
                 FROM material",
            ))
            .await?
            .ok_or_else(|| DbErr::Custom("Postgres returned no invitation material".into()))?;
        Ok(RandomMaterial {
            id: row.try_get("", "id")?,
            token: row.try_get("", "token")?,
            token_hash: row.try_get("", "token_hash")?,
            expires_at: row.try_get("", "expires_at")?,
        })
    }

    fn now() -> DateTimeWithTimeZone {
        let utc: DateTimeUtc = SystemTime::now().into();
        utc.fixed_offset()
    }

    fn empty_json() -> Json {
        Json::Object(Default::default())
    }

    fn require_admin(role: &str) -> Result<(), DbErr> {
        if matches!(role, "owner" | "admin") {
            Ok(())
        } else {
            Err(DbErr::Custom("administrator role required".into()))
        }
    }

    fn validate_invite_role(role: &str) -> Result<(), DbErr> {
        if matches!(role, "admin" | "member" | "reader") {
            Ok(())
        } else {
            Err(DbErr::Custom("invalid invitation role".into()))
        }
    }

    pub(crate) fn validate_visibility(visibility: &str) -> Result<(), DbErr> {
        if matches!(visibility, "public" | "private" | "internal") {
            Ok(())
        } else {
            Err(DbErr::Custom("invalid package visibility".into()))
        }
    }

    pub(crate) fn validate_slug(slug: &str) -> Result<(), DbErr> {
        if slug.len() >= 2
            && slug.len() <= 64
            && slug
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && !slug.starts_with('-')
            && !slug.ends_with('-')
        {
            Ok(())
        } else {
            Err(DbErr::Custom("invalid slug".into()))
        }
    }

    pub(crate) fn normalize_email(email: &str) -> Result<String, DbErr> {
        let email = email.trim().to_lowercase();
        if email.len() <= 320
            && email.contains('@')
            && !email.bytes().any(|byte| byte.is_ascii_whitespace())
        {
            Ok(email)
        } else {
            Err(DbErr::Custom("invalid email".into()))
        }
    }

    fn user_summary(model: user::Model) -> UserSummary {
        UserSummary {
            id: model.id,
            subject: model.shared_auth_subject,
            email: model.email,
            display_name: model.display_name,
            avatar_url: model.avatar_url,
            settings: model.settings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::write::{normalize_email, validate_slug, validate_visibility};

    #[test]
    fn validation_is_fail_closed() {
        assert!(validate_slug("project-one").is_ok());
        assert!(validate_slug("../admin").is_err());
        assert!(validate_visibility("private").is_ok());
        assert!(validate_visibility("world").is_err());
        assert_eq!(
            normalize_email(" User@Example.COM ").unwrap(),
            "user@example.com"
        );
        assert!(normalize_email("not-an-email").is_err());
    }
}
