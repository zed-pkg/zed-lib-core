use sea_orm::entity::prelude::*;

/// `zed_package_downloads`. Append-only. Inserting a row bumps the package and
/// version counters through a database trigger, so callers must not also
/// increment them by hand.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_package_downloads")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub package_id: Uuid,
    pub package_version_id: Option<Uuid>,
    pub downloaded_by_user_id: Option<Uuid>,
    pub api_token_id: Option<Uuid>,
    pub source: String,
    pub format: Option<String>,
    pub bytes_sent: Option<i64>,
    pub client_ip_hash: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTimeWithTimeZone,
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
