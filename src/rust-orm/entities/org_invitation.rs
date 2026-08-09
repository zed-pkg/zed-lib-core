use sea_orm::entity::prelude::*;

/// `zed_org_invitations`. Only the SHA-256 of the invite token is durable.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_org_invitations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub org_id: Uuid,
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
        belongs_to = "super::org::Entity",
        from = "Column::OrgId",
        to = "super::org::Column::Id"
    )]
    Org,
}

impl Related<super::org::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Org.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
