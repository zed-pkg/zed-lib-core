use sea_orm::entity::prelude::*;

/// `zed_project_invitations`. Only the SHA-256 of the invite token is durable.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_project_invitations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub project_id: Uuid,
    pub invited_by_user_id: Uuid,
    pub email: String,
    pub role: String,
    pub token_hash: String,
    pub expires_at: DateTimeWithTimeZone,
    pub accepted_at: Option<DateTimeWithTimeZone>,
    pub accepted_by_user_id: Option<Uuid>,
    pub revoked_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id"
    )]
    Project,
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
