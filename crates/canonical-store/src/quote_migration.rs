use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

/// Quote intake has an independent migration ledger so the established
/// canonical-store migration sequence remains immutable. The application
/// migration command runs both ledgers explicitly.
pub struct QuoteMigrator;

#[async_trait::async_trait]
impl MigratorTrait for QuoteMigrator {
    fn migration_table_name() -> DynIden {
        Alias::new("canonical_quote_migrations").into_iden()
    }

    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(AddQuoteIntake)]
    }
}

#[derive(DeriveMigrationName)]
struct AddQuoteIntake;

#[async_trait::async_trait]
impl MigrationTrait for AddQuoteIntake {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CanonicalContext::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CanonicalContext::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(CanonicalContext::Scope).text().not_null())
                    .col(
                        ColumnDef::new(CanonicalContext::ContentMarkdown)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CanonicalContext::Version)
                            .big_integer()
                            .not_null()
                            .check(Expr::col(CanonicalContext::Version).gt(0)),
                    )
                    .col(
                        ColumnDef::new(CanonicalContext::Active)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(CanonicalContext::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CanonicalContext::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("canonical_context_scope_version_uidx")
                    .table(CanonicalContext::Table)
                    .col(CanonicalContext::Scope)
                    .col(CanonicalContext::Version)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("canonical_context_active_lookup_idx")
                    .table(CanonicalContext::Table)
                    .col(CanonicalContext::Scope)
                    .col(CanonicalContext::Active)
                    .col(CanonicalContext::Version)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(QuoteRequest::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(QuoteRequest::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(QuoteRequest::OwnerSubject).text().not_null())
                    .col(ColumnDef::new(QuoteRequest::OwnerEmail).string())
                    .col(
                        ColumnDef::new(QuoteRequest::CompanyName)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(QuoteRequest::Website).string())
                    .col(
                        ColumnDef::new(QuoteRequest::EmployeeCount)
                            .integer()
                            .not_null()
                            .check(Expr::col(QuoteRequest::EmployeeCount).between(1, 1_000_000)),
                    )
                    .col(
                        ColumnDef::new(QuoteRequest::Frameworks)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(QuoteRequest::CloudProviders)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(QuoteRequest::SensitiveData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(QuoteRequest::CurrentControls)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(QuoteRequest::TargetTimeline)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(QuoteRequest::Notes).text())
                    .col(
                        ColumnDef::new(QuoteRequest::Status)
                            .text()
                            .not_null()
                            .check(Expr::col(QuoteRequest::Status).is_in([
                                "queued",
                                "analyzing",
                                "ready",
                                "failed",
                                "cancelled",
                            ])),
                    )
                    .col(
                        ColumnDef::new(QuoteRequest::RequestPayload)
                            .json_binary()
                            .not_null(),
                    )
                    .col(ColumnDef::new(QuoteRequest::AnalysisPayload).json_binary())
                    .col(ColumnDef::new(QuoteRequest::EstimateLowCents).big_integer())
                    .col(ColumnDef::new(QuoteRequest::EstimateHighCents).big_integer())
                    .col(
                        ColumnDef::new(QuoteRequest::Currency)
                            .string()
                            .not_null()
                            .default("USD"),
                    )
                    .col(ColumnDef::new(QuoteRequest::ModelName).string())
                    .col(ColumnDef::new(QuoteRequest::ContextVersion).big_integer())
                    .col(ColumnDef::new(QuoteRequest::FailureCode).string())
                    .col(
                        ColumnDef::new(QuoteRequest::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(QuoteRequest::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("quote_request_owner_created_idx")
                    .table(QuoteRequest::Table)
                    .col(QuoteRequest::OwnerSubject)
                    .col(QuoteRequest::CreatedAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("quote_request_owner_status_idx")
                    .table(QuoteRequest::Table)
                    .col(QuoteRequest::OwnerSubject)
                    .col(QuoteRequest::Status)
                    .to_owned(),
            )
            .await?;

        if manager.get_database_backend() == DatabaseBackend::Postgres {
            manager
                .get_connection()
                .execute_unprepared(
                    r#"
                    ALTER TABLE canonical_context ENABLE ROW LEVEL SECURITY;
                    ALTER TABLE canonical_context FORCE ROW LEVEL SECURITY;
                    ALTER TABLE quote_request ENABLE ROW LEVEL SECURITY;
                    ALTER TABLE quote_request FORCE ROW LEVEL SECURITY;

                    CREATE POLICY canonical_context_authenticated_reader
                      ON canonical_context FOR SELECT
                      USING (
                        active
                        AND NULLIF(
                          current_setting('canonical.shared_auth_sub', true),
                          ''
                        ) IS NOT NULL
                      );

                    CREATE POLICY quote_request_shared_auth_owner
                      ON quote_request
                      USING (
                        owner_subject = NULLIF(
                          current_setting('canonical.shared_auth_sub', true),
                          ''
                        )
                      )
                      WITH CHECK (
                        owner_subject = NULLIF(
                          current_setting('canonical.shared_auth_sub', true),
                          ''
                        )
                      );
                    "#,
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(QuoteRequest::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(CanonicalContext::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CanonicalContext {
    Table,
    Id,
    Scope,
    ContentMarkdown,
    Version,
    Active,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum QuoteRequest {
    Table,
    Id,
    OwnerSubject,
    OwnerEmail,
    CompanyName,
    Website,
    EmployeeCount,
    Frameworks,
    CloudProviders,
    SensitiveData,
    CurrentControls,
    TargetTimeline,
    Notes,
    Status,
    RequestPayload,
    AnalysisPayload,
    EstimateLowCents,
    EstimateHighCents,
    Currency,
    ModelName,
    ContextVersion,
    FailureCode,
    CreatedAt,
    UpdatedAt,
}
