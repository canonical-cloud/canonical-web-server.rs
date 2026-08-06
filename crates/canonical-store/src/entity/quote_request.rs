use sea_orm::entity::prelude::*;

pub const STATUSES: [&str; 5] = ["queued", "analyzing", "ready", "failed", "cancelled"];

/// One customer-owned quote request. The owner is the opaque Shared Auth
/// subject, not a provider-specific UUID, so Canonical can migrate identity
/// providers without rewriting quote ownership.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "quote_request")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub owner_subject: String,
    pub owner_email: Option<String>,
    pub company_name: String,
    pub website: Option<String>,
    pub employee_count: i32,
    pub frameworks: Json,
    pub cloud_providers: Json,
    pub sensitive_data: Json,
    pub current_controls: String,
    pub target_timeline: String,
    pub notes: Option<String>,
    pub status: String,
    pub request_payload: Json,
    pub analysis_payload: Option<Json>,
    pub estimate_low_cents: Option<i64>,
    pub estimate_high_cents: Option<i64>,
    pub currency: String,
    pub model_name: Option<String>,
    pub context_version: Option<i64>,
    pub failure_code: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
