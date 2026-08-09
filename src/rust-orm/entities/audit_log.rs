use sea_orm::entity::prelude::*;

/// `zed_audit_log`. Append-only.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "zed_audit_log")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub org_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub api_token_id: Option<Uuid>,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<Uuid>,
    #[sea_orm(column_type = "JsonBinary")]
    pub detail: Json,
    pub client_ip_hash: Option<String>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
