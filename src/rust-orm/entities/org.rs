use sea_orm::entity::prelude::*;

/// `zed_orgs`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_orgs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    #[sea_orm(column_type = "JsonBinary")]
    pub settings: Json,
    pub created_by_user_id: Option<Uuid>,
    pub is_soft_deleted: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::project::Entity")]
    Project,
    #[sea_orm(has_many = "super::package::Entity")]
    Package,
    #[sea_orm(has_many = "super::org_member::Entity")]
    OrgMember,
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl Related<super::package::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Package.def()
    }
}

impl Related<super::org_member::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::OrgMember.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
