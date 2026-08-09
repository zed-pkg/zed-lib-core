use sea_orm::entity::prelude::*;

/// `zed_package_versions`. `artifact_key` addresses the object in R2.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_package_versions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub package_id: Uuid,
    pub version: String,
    pub version_scheme: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub format: String,
    pub vcs_tag: Option<String>,
    pub vcs_commit: Option<String>,
    pub artifact_key: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub manifest: Json,
    pub download_count: i64,
    pub yanked: bool,
    pub yanked_at: Option<DateTimeWithTimeZone>,
    pub yanked_reason: Option<String>,
    pub published_by_user_id: Option<Uuid>,
    pub published_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::package::Entity",
        from = "Column::PackageId",
        to = "super::package::Column::Id"
    )]
    Package,
}

impl Related<super::package::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Package.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
