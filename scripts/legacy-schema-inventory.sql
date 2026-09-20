\set ON_ERROR_STOP on
\pset pager off
\pset tuples_only off
\pset format aligned

\echo '=== Canonical legacy web-store extraction inventory ==='
\echo 'This script is read-only. Run it with a credential that can inspect the legacy customer web database.'

SELECT
  current_database() AS database_name,
  current_user AS current_role,
  session_user AS login_role,
  current_schema() AS current_schema;

SELECT
  rolname AS current_role,
  rolsuper AS is_superuser,
  rolbypassrls AS bypasses_rls,
  rolcreatedb AS can_create_database,
  rolcreaterole AS can_create_role,
  rolreplication AS can_replicate
FROM pg_catalog.pg_roles
WHERE rolname = current_user;

\echo '=== Expected legacy/current relations ==='
WITH expected(table_name, ownership_class) AS (
  VALUES
    ('user_profile', 'legacy_web_identity'),
    ('web_session', 'legacy_web_session'),
    ('sync_record', 'legacy_web_sync'),
    ('sync_clock', 'legacy_web_sync'),
    ('sync_change', 'legacy_web_sync'),
    ('sync_receipt', 'legacy_web_sync'),
    ('admin_role_assignment', 'admin_plane_must_extract'),
    ('admin_audit_event', 'admin_plane_must_extract'),
    ('audit_engagement', 'legacy_singular_engagement'),
    ('engagement_note', 'legacy_singular_engagement'),
    ('audit_engagements', 'canonical_audit_plural')
)
SELECT
  expected.ownership_class,
  expected.table_name,
  c.oid IS NOT NULL AS present,
  COALESCE(n.nspname, '') AS schema_name,
  COALESCE(pg_catalog.pg_get_userbyid(c.relowner), '') AS owner_role,
  COALESCE(c.relrowsecurity, false) AS row_level_security,
  COALESCE(c.relforcerowsecurity, false) AS force_row_level_security
FROM expected
LEFT JOIN pg_catalog.pg_namespace n
  ON n.nspname = 'public'
LEFT JOIN pg_catalog.pg_class c
  ON c.relnamespace = n.oid
 AND c.relname = expected.table_name
 AND c.relkind IN ('r', 'p')
ORDER BY expected.ownership_class, expected.table_name;

\echo '=== Row counts for relations that actually exist ==='
SELECT format(
  'SELECT %L AS ownership_class, %L AS table_name, count(*)::bigint AS row_count FROM public.%I;',
  expected.ownership_class,
  expected.table_name,
  expected.table_name
)
FROM (
  VALUES
    ('user_profile', 'legacy_web_identity'),
    ('web_session', 'legacy_web_session'),
    ('sync_record', 'legacy_web_sync'),
    ('sync_clock', 'legacy_web_sync'),
    ('sync_change', 'legacy_web_sync'),
    ('sync_receipt', 'legacy_web_sync'),
    ('admin_role_assignment', 'admin_plane_must_extract'),
    ('admin_audit_event', 'admin_plane_must_extract'),
    ('audit_engagement', 'legacy_singular_engagement'),
    ('engagement_note', 'legacy_singular_engagement'),
    ('audit_engagements', 'canonical_audit_plural')
) AS expected(table_name, ownership_class)
WHERE pg_catalog.to_regclass(format('public.%I', expected.table_name)) IS NOT NULL
ORDER BY expected.ownership_class, expected.table_name
\gexec

\echo '=== Legacy admin functions that must not remain customer-plane authority ==='
WITH expected(function_name) AS (
  VALUES
    ('canonical_admin_has_capability'),
    ('canonical_admin_append_audit')
)
SELECT
  expected.function_name,
  p.oid IS NOT NULL AS present,
  COALESCE(pg_catalog.pg_get_userbyid(p.proowner), '') AS owner_role,
  COALESCE(p.prosecdef, false) AS security_definer,
  COALESCE(pg_catalog.pg_get_function_identity_arguments(p.oid), '') AS identity_arguments
FROM expected
LEFT JOIN pg_catalog.pg_namespace n
  ON n.nspname = 'public'
LEFT JOIN pg_catalog.pg_proc p
  ON p.pronamespace = n.oid
 AND p.proname = expected.function_name
ORDER BY expected.function_name, identity_arguments;

\echo '=== Legacy SeaORM migration ledger, if present ==='
SELECT format(
  'SELECT version, applied_at FROM public.seaql_migrations ORDER BY version;'
)
WHERE pg_catalog.to_regclass('public.seaql_migrations') IS NOT NULL
\gexec

\echo '=== canonical-orm-core migration ledger, if present ==='
SELECT format(
  'SELECT name, applied_at FROM public.canonical_orm_migrations ORDER BY name;'
)
WHERE pg_catalog.to_regclass('public.canonical_orm_migrations') IS NOT NULL
\gexec

\echo '=== Extraction blockers ==='
WITH relation_state AS (
  SELECT
    pg_catalog.to_regclass('public.admin_role_assignment') IS NOT NULL AS has_admin_roles,
    pg_catalog.to_regclass('public.admin_audit_event') IS NOT NULL AS has_admin_audit,
    pg_catalog.to_regclass('public.audit_engagement') IS NOT NULL AS has_legacy_engagement,
    pg_catalog.to_regclass('public.audit_engagements') IS NOT NULL AS has_canonical_engagement
), function_state AS (
  SELECT EXISTS (
    SELECT 1
    FROM pg_catalog.pg_proc p
    JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
    WHERE n.nspname = 'public'
      AND p.proname IN ('canonical_admin_has_capability', 'canonical_admin_append_audit')
  ) AS has_admin_functions
)
SELECT
  has_admin_roles OR has_admin_audit OR has_admin_functions AS admin_plane_objects_still_in_customer_database,
  has_legacy_engagement AS legacy_singular_engagement_present,
  has_canonical_engagement AS canonical_plural_engagement_present,
  has_legacy_engagement AND has_canonical_engagement AS dual_engagement_models_present
FROM relation_state CROSS JOIN function_state;

\echo 'Inventory complete. Do not drop, rename, or backfill anything from this output alone.'
