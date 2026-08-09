use sea_orm::entity::prelude::*;

/// `zed_packages`.
///
/// `download_count` is maintained by the `zed_package_downloads` trigger and is
/// the authoritative input to the private→public promotion rule — never write
/// it from application code.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_packages")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub vcs: String,
    pub repo_url: String,
    pub homepage_url: Option<String>,
    #[sea_orm(column_type = "JsonBinary")]
    pub keywords: Json,
    #[sea_orm(column_type = "JsonBinary")]
    pub config: Json,
    pub download_count: i64,
    pub version_count: i32,
    pub latest_version: Option<String>,
    pub first_published_at: Option<DateTimeWithTimeZone>,
    pub visibility_changed_at: Option<DateTimeWithTimeZone>,
    pub created_by_user_id: Option<Uuid>,
    pub is_soft_deleted: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::org::Entity",
        from = "Column::OrgId",
        to = "super::org::Column::Id"
    )]
    Org,
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id"
    )]
    Project,
    #[sea_orm(has_many = "super::package_version::Entity")]
    PackageVersion,
}

impl Related<super::org::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Org.def()
    }
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl Related<super::package_version::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PackageVersion.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
