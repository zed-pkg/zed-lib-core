use sea_orm::entity::prelude::*;

/// Immutable, content-addressed dependency graph document.
///
/// `document` is the lossless authority. The related edge rows are a
/// relational index derived from the same document and committed atomically.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_dependency_graph_artifacts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub root_package_version_id: Uuid,
    pub graph_kind: String,
    pub schema_version: String,
    pub graph_digest: String,
    pub resolver_name: Option<String>,
    pub resolver_version: Option<String>,
    pub resolution_input_digest: Option<String>,
    pub registry_checkpoint: Option<String>,
    #[sea_orm(column_type = "JsonBinary")]
    pub target: Json,
    #[sea_orm(column_type = "JsonBinary")]
    pub enabled_features: Json,
    #[sea_orm(column_type = "JsonBinary")]
    pub document: Json,
    pub node_count: i32,
    pub edge_count: i32,
    pub max_depth: i32,
    pub cycle_count: i32,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::package_version::Entity",
        from = "Column::RootPackageVersionId",
        to = "super::package_version::Column::Id"
    )]
    RootPackageVersion,
    #[sea_orm(has_many = "super::dependency_graph_edge::Entity")]
    Edge,
}

impl Related<super::package_version::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RootPackageVersion.def()
    }
}

impl Related<super::dependency_graph_edge::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Edge.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
