//! SeaORM connection and explicit migration entry points.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::{db::migration::Migrator, error::AppError};

pub async fn connect(
    database_url: &str,
    database_max_connections: u32,
) -> Result<sea_orm::DatabaseConnection, sea_orm::DbErr> {
    crate::db::connect_database(database_url, database_max_connections).await
}

/// Applies all legacy web/session/sync migrations and exits without
/// constructing HTTP or Supabase clients.
///
/// PostgreSQL migrations require a direct, explicitly named ordinary migrator
/// identity. Superuser/BYPASSRLS/elevated/member/SET ROLE sessions are refused
/// before any DDL executes. SQLite remains available for isolated local tests.
pub async fn run_migrations(
    database_url: &str,
    database_max_connections: u32,
) -> Result<(), AppError> {
    let db = connect(database_url, database_max_connections).await?;
    if db.get_database_backend() == DatabaseBackend::Postgres {
        let expected_role = std::env::var("MIGRATION_EXPECTED_ROLE").map_err(|_| {
            AppError::Configuration(
                "MIGRATION_EXPECTED_ROLE is required for PostgreSQL legacy web migrations",
            )
        })?;
        if expected_role.trim().is_empty() {
            return Err(AppError::Configuration(
                "MIGRATION_EXPECTED_ROLE must not be empty",
            ));
        }
        verify_migration_database_role(&db, expected_role.trim()).await?;
    }
    Migrator::up(&db, None).await?;
    db.close().await?;
    Ok(())
}

async fn verify_migration_database_role(
    db: &sea_orm::DatabaseConnection,
    expected_role: &str,
) -> Result<(), AppError> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
            SELECT
              current_user::text AS current_role,
              session_user::text AS login_role,
              r.rolsuper AS is_superuser,
              r.rolbypassrls AS bypasses_rls,
              r.rolcreaterole AS can_create_role,
              r.rolcreatedb AS can_create_database,
              r.rolreplication AS can_replicate,
              EXISTS (
                SELECT 1
                FROM pg_catalog.pg_auth_members memberships
                WHERE memberships.member = r.oid
              ) AS has_memberships
            FROM pg_catalog.pg_roles r
            WHERE r.rolname = current_user
            "#
            .to_owned(),
        ))
        .await?
        .ok_or(AppError::Configuration(
            "database did not return the legacy migration principal",
        ))?;

    let current_role = row.try_get::<String>("", "current_role")?;
    let login_role = row.try_get::<String>("", "login_role")?;
    if current_role != expected_role || login_role != expected_role {
        return Err(AppError::Configuration(
            "legacy migration login/current role does not match MIGRATION_EXPECTED_ROLE",
        ));
    }

    if let Some(message) = migration_principal_refusal(
        row.try_get::<bool>("", "is_superuser")?,
        row.try_get::<bool>("", "bypasses_rls")?,
        row.try_get::<bool>("", "can_create_role")?,
        row.try_get::<bool>("", "can_create_database")?,
        row.try_get::<bool>("", "can_replicate")?,
        row.try_get::<bool>("", "has_memberships")?,
        &login_role,
        &current_role,
    ) {
        return Err(AppError::Configuration(message));
    }

    Ok(())
}

fn migration_principal_refusal(
    is_superuser: bool,
    bypasses_rls: bool,
    can_create_role: bool,
    can_create_database: bool,
    can_replicate: bool,
    has_memberships: bool,
    login_role: &str,
    current_role: &str,
) -> Option<&'static str> {
    if is_superuser {
        return Some("legacy migrations refuse PostgreSQL SUPERUSER credentials");
    }
    if bypasses_rls {
        return Some("legacy migrations refuse PostgreSQL BYPASSRLS credentials");
    }
    if can_create_role || can_create_database || can_replicate {
        return Some(
            "legacy migrations refuse CREATEROLE, CREATEDB, or REPLICATION credentials",
        );
    }
    if has_memberships {
        return Some("legacy migrations refuse credentials that inherit another PostgreSQL role");
    }
    if login_role != current_role {
        return Some("legacy migrations refuse SET ROLE sessions");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::migration_principal_refusal;

    #[test]
    fn legacy_migrator_principal_is_fail_closed() {
        assert_eq!(
            migration_principal_refusal(
                false,
                false,
                false,
                false,
                false,
                false,
                "canonical_web_migrator",
                "canonical_web_migrator",
            ),
            None
        );
        assert!(migration_principal_refusal(
            true,
            false,
            false,
            false,
            false,
            false,
            "postgres",
            "postgres",
        )
        .is_some());
        assert!(migration_principal_refusal(
            false,
            true,
            false,
            false,
            false,
            false,
            "migrator",
            "migrator",
        )
        .is_some());
        assert!(migration_principal_refusal(
            false,
            false,
            false,
            false,
            false,
            true,
            "migrator",
            "migrator",
        )
        .is_some());
        assert!(migration_principal_refusal(
            false,
            false,
            false,
            false,
            false,
            false,
            "login",
            "delegated",
        )
        .is_some());
    }
}
