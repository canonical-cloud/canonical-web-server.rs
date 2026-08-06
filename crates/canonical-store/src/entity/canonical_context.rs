use sea_orm::entity::prelude::*;

/// Versioned analysis context owned by the reviewed database migration/content
/// workflow. Customer-facing processes receive SELECT only and cannot mutate
/// prompt instructions at runtime.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "canonical_context")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub scope: String,
    pub content_markdown: String,
    pub version: i64,
    pub active: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
