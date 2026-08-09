use sea_orm::entity::prelude::*;

/// `zed_users`. `shared_auth_subject` + `auth_realm` address a shared_auth
/// principal on a *different* RDS instance, so there is no foreign key to it.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub shared_auth_subject: Uuid,
    pub auth_realm: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    #[sea_orm(column_type = "JsonBinary")]
    pub settings: Json,
    pub is_soft_deleted: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::org_member::Entity")]
    OrgMember,
    #[sea_orm(has_many = "super::project_member::Entity")]
    ProjectMember,
}

impl Related<super::org_member::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::OrgMember.def()
    }
}

impl Related<super::project_member::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ProjectMember.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
