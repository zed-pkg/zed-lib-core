use sea_orm::entity::prelude::*;

/// Normalized edge derived from one immutable dependency graph document.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_dependency_graph_edges")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub graph_artifact_id: Uuid,
    pub ordinal: i32,
    pub from_registry_id: String,
    pub from_org_slug: String,
    pub from_package_name: String,
    pub from_version: Option<String>,
    pub from_package_id: Option<Uuid>,
    pub from_package_version_id: Option<Uuid>,
    pub to_registry_id: String,
    pub to_org_slug: String,
    pub to_package_name: String,
    pub to_version: Option<String>,
    pub to_package_id: Option<Uuid>,
    pub to_package_version_id: Option<Uuid>,
    pub requirement: Option<String>,
    pub dependency_kind: String,
    pub optional: bool,
    pub default_features: bool,
    #[sea_orm(column_type = "JsonBinary")]
    pub features: Json,
    pub target: Option<String>,
    pub minimum_depth: i32,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::dependency_graph_artifact::Entity",
        from = "Column::GraphArtifactId",
        to = "super::dependency_graph_artifact::Column::Id"
    )]
    GraphArtifact,
    #[sea_orm(
        belongs_to = "super::package::Entity",
        from = "Column::FromPackageId",
        to = "super::package::Column::Id"
    )]
    FromPackage,
    #[sea_orm(
        belongs_to = "super::package_version::Entity",
        from = "Column::FromPackageVersionId",
        to = "super::package_version::Column::Id"
    )]
    FromPackageVersion,
    #[sea_orm(
        belongs_to = "super::package::Entity",
        from = "Column::ToPackageId",
        to = "super::package::Column::Id"
    )]
    ToPackage,
    #[sea_orm(
        belongs_to = "super::package_version::Entity",
        from = "Column::ToPackageVersionId",
        to = "super::package_version::Column::Id"
    )]
    ToPackageVersion,
}

impl Related<super::dependency_graph_artifact::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GraphArtifact.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
