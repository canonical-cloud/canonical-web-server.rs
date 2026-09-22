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

    let principal = MigrationPrincipalFacts {
        is_superuser: row.try_get::<bool>("", "is_superuser")?,
        bypasses_rls: row.try_get::<bool>("", "bypasses_rls")?,
        can_create_role: row.try_get::<bool>("", "can_create_role")?,
        can_create_database: row.try_get::<bool>("", "can_create_database")?,
        can_replicate: row.try_get::<bool>("", "can_replicate")?,
        has_memberships: row.try_get::<bool>("", "has_memberships")?,
        login_role: &login_role,
        current_role: &current_role,
    };
    if let Some(message) = migration_principal_refusal(principal) {
        return Err(AppError::Configuration(message));
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MigrationPrincipalFacts<'a> {
    is_superuser: bool,
    bypasses_rls: bool,
    can_create_role: bool,
    can_create_database: bool,
    can_replicate: bool,
    has_memberships: bool,
    login_role: &'a str,
    current_role: &'a str,
}

fn migration_principal_refusal(principal: MigrationPrincipalFacts<'_>) -> Option<&'static str> {
    if principal.is_superuser {
        return Some("legacy migrations refuse PostgreSQL SUPERUSER credentials");
    }
    if principal.bypasses_rls {
        return Some("legacy migrations refuse PostgreSQL BYPASSRLS credentials");
    }
    if principal.can_create_role || principal.can_create_database || principal.can_replicate {
        return Some("legacy migrations refuse CREATEROLE, CREATEDB, or REPLICATION credentials");
    }
    if principal.has_memberships {
        return Some("legacy migrations refuse credentials that inherit another PostgreSQL role");
    }
    if principal.login_role != principal.current_role {
        return Some("legacy migrations refuse SET ROLE sessions");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{migration_principal_refusal, MigrationPrincipalFacts};

    fn ordinary_migrator<'a>(role: &'a str) -> MigrationPrincipalFacts<'a> {
        MigrationPrincipalFacts {
            is_superuser: false,
            bypasses_rls: false,
            can_create_role: false,
            can_create_database: false,
            can_replicate: false,
            has_memberships: false,
            login_role: role,
            current_role: role,
        }
    }

    #[test]
    fn legacy_migrator_principal_is_fail_closed() {
        assert_eq!(
            migration_principal_refusal(ordinary_migrator("canonical_web_migrator")),
            None
        );

        let mut principal = ordinary_migrator("postgres");
        principal.is_superuser = true;
        assert!(migration_principal_refusal(principal).is_some());

        let mut principal = ordinary_migrator("migrator");
        principal.bypasses_rls = true;
        assert!(migration_principal_refusal(principal).is_some());

        let mut principal = ordinary_migrator("migrator");
        principal.has_memberships = true;
        assert!(migration_principal_refusal(principal).is_some());

        let mut principal = ordinary_migrator("login");
        principal.current_role = "delegated";
        assert!(migration_principal_refusal(principal).is_some());
    }
}
