use sea_orm::entity::prelude::*;

/// `zed_package_licenses`. A row with `package_version_id == None` is the
/// package-level default for versions that declare nothing of their own.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_package_licenses")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub package_id: Uuid,
    pub package_version_id: Option<Uuid>,
    pub kind: String,
    pub spdx_id: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
    pub text_body: Option<String>,
    pub is_primary: bool,
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
