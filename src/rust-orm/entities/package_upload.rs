use sea_orm::entity::prelude::*;

/// `zed_package_uploads`. The publish-attempt record: created before bytes
/// reach R2 and retained for failed/aborted attempts, so it is deliberately not
/// one-to-one with [`super::package_version`].
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_package_uploads")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub package_id: Uuid,
    pub package_version_id: Option<Uuid>,
    pub requested_version: String,
    pub status: String,
    pub storage_backend: String,
    pub storage_key: Option<String>,
    pub format: Option<String>,
    pub size_bytes: Option<i64>,
    pub sha256: Option<String>,
    pub uploaded_by_user_id: Option<Uuid>,
    pub api_token_id: Option<Uuid>,
    pub client_ip_hash: Option<String>,
    pub user_agent: Option<String>,
    pub error: Option<String>,
    pub started_at: DateTimeWithTimeZone,
    pub completed_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
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
