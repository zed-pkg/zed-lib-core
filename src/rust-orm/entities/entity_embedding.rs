use sea_orm::entity::prelude::*;

/// `zed_entity_embeddings`. Polymorphic over `entity_type`, so `entity_id` is
/// not foreign-keyed; the writer deletes embeddings alongside their entity.
/// Vectors are a jsonb array plus an explicit dimension count — the canonical
/// contract deliberately does not require the `vector` extension.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_entity_embeddings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub org_id: Option<Uuid>,
    pub embedding_model: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub embedding: Json,
    pub embedding_dimensions: i32,
    pub content_sha256: String,
    pub content_preview: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
