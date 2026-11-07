//! GW-I-016: migrations remain forward-only, repeatable and data-preserving, from empty and from the previous release.

use sqlx::PgPool;
use tqa_gateway::MIGRATOR;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[sqlx::test(migrations = false)]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_016_migrations_preserve_data_when_reapplied(pool: PgPool) -> TestResult {
    require_database()?;
    MIGRATOR.run(&pool).await?;
    let id = uuid::Uuid::now_v7();
    sqlx::query!("INSERT INTO users (id, email, password_hash, role) VALUES ($1, 'migration@example.invalid', 'fixture-hash', 'admin')", id)
        .execute(&pool).await?;
    MIGRATOR.run(&pool).await?;
    let found = sqlx::query_scalar!("SELECT id FROM users WHERE id = $1", id)
        .fetch_one(&pool)
        .await?;
    assert_eq!(found, id);
    let migrations =
        sqlx::query_scalar!("SELECT count(*) FROM _sqlx_migrations WHERE success = true")
            .fetch_one(&pool)
            .await?;
    // Every embedded migration applied once: counting them keeps parallel branches that each add one from conflicting.
    assert_eq!(migrations, Some(i64::try_from(MIGRATOR.migrations.len())?));
    assert!(
        MIGRATOR.migrations.len() >= 38,
        "the embedded migrations are all present"
    );
    Ok(())
}

fn require_database() -> TestResult {
    let expected = std::env::var("TQA_DATABASE_URL")
        .map_err(|_| "PostgreSQL tests require TQA_DATABASE_URL and DATABASE_URL")?;
    if expected.is_empty()
        || std::env::var("DATABASE_URL").ok().as_deref() != Some(expected.as_str())
    {
        return Err("DATABASE_URL must equal TQA_DATABASE_URL for the SQLx test harness".into());
    }
    Ok(())
}

/// The previous release's database as synthetic rows in every table; its first line names the release's last migration.
const PREVIOUS_RELEASE: &str = include_str!("fixtures/upgrade-previous-release.sql");

/// Tables whose existing rows a migration after the fixture's level changes or drops on purpose, each with that
/// migration's version. Every other table keeps its rows exactly; an entry goes once the fixture's level passes it.
const REWRITTEN: &[(&str, i64)] = &[];

fn fixture_level() -> Result<i64, Box<dyn std::error::Error>> {
    let first = PREVIOUS_RELEASE.lines().next().unwrap_or_default();
    let level = first
        .strip_prefix("-- level: ")
        .ok_or("the fixture's first line is `-- level: <the previous release's last migration>`")?;
    Ok(level.trim().parse()?)
}

#[test]
fn gw_i_016_the_upgrade_fixture_starts_at_an_embedded_migration() -> TestResult {
    let level = fixture_level()?;
    let versions: Vec<i64> = MIGRATOR
        .migrations
        .iter()
        .map(|migration| migration.version)
        .collect();
    assert!(
        versions.contains(&level),
        "the fixture's level {level} is not an embedded migration"
    );
    for (table, version) in REWRITTEN {
        assert!(
            *version > level && versions.contains(version),
            "{table}: migration {version} is not an embedded migration after the fixture's level {level}"
        );
    }
    Ok(())
}

#[sqlx::test(migrations = false)]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_016_an_upgrade_from_the_previous_release_keeps_every_row(pool: PgPool) -> TestResult {
    require_database()?;
    // A fresh install's schema first, then the same database emptied: both come from one server and one owner.
    MIGRATOR.run(&pool).await?;
    let fresh = schema(&pool).await?;
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public")
        .execute(&pool)
        .await?;

    MIGRATOR.run_to(fixture_level()?, &pool).await?;
    sqlx::raw_sql(PREVIOUS_RELEASE).execute(&pool).await?;
    let before = rows(&pool).await?;
    let empty: Vec<&str> = before
        .iter()
        .filter(|table| table.count == 0)
        .map(|table| table.name.as_str())
        .collect();
    assert!(
        empty.is_empty(),
        "every table at the previous release's level needs a fixture row; none in {empty:?}"
    );

    MIGRATOR.run(&pool).await?;
    for table in &before {
        if REWRITTEN.iter().any(|(name, _)| *name == table.name) {
            continue;
        }
        // Only the columns the table had before the upgrade: a new column's default isn't a change to a row.
        let after = table_rows(&pool, &table.quoted, &table.columns).await?;
        assert_eq!(
            after,
            (table.count, table.digest.clone()),
            "{}: the upgrade changed its rows",
            table.name
        );
    }
    let applied: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success = true")
            .fetch_one(&pool)
            .await?;
    assert_eq!(applied, i64::try_from(MIGRATOR.migrations.len())?);

    let upgraded = schema(&pool).await?;
    let only_fresh: Vec<&String> = fresh
        .iter()
        .filter(|line| !upgraded.contains(line))
        .collect();
    let only_upgraded: Vec<&String> = upgraded
        .iter()
        .filter(|line| !fresh.contains(line))
        .collect();
    assert!(
        only_fresh.is_empty() && only_upgraded.is_empty(),
        "the upgraded schema differs from a fresh install's: only in the fresh one {only_fresh:#?}, \
         only in the upgraded one {only_upgraded:#?}"
    );
    Ok(())
}

/// One table as the previous release left it: its rows' count and a digest over the columns it had then.
struct Table {
    name: String,
    quoted: String,
    columns: String,
    count: i64,
    digest: String,
}

async fn rows(pool: &PgPool) -> Result<Vec<Table>, sqlx::Error> {
    let tables: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT c.relname::text, quote_ident(c.relname), \
                string_agg(quote_ident(a.attname), ', ' ORDER BY a.attnum) \
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
         WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') AND c.relname <> '_sqlx_migrations' \
         GROUP BY c.relname ORDER BY c.relname",
    )
    .fetch_all(pool)
    .await?;
    let mut found = Vec::with_capacity(tables.len());
    for (name, quoted, columns) in tables {
        let (count, digest) = table_rows(pool, &quoted, &columns).await?;
        found.push(Table {
            name,
            quoted,
            columns,
            count,
            digest,
        });
    }
    Ok(found)
}

/// A table's row count and a digest of its rows' values in `columns`; both names come quoted from the catalogue.
async fn table_rows(
    pool: &PgPool,
    quoted: &str,
    columns: &str,
) -> Result<(i64, String), sqlx::Error> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT count(*), md5(coalesce(string_agg(r, E'\\n' ORDER BY r), '')) \
         FROM (SELECT ROW({columns})::text AS r FROM public.{quoted}) AS t"
    )))
    .fetch_one(pool)
    .await
}

/// The schema as sorted lines: every column, constraint, index, trigger, function and extension.
async fn schema(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT line FROM ( \
           SELECT format('column %s.%s %s%s%s%s%s', c.relname, a.attname, format_type(a.atttypid, a.atttypmod), \
                  CASE WHEN a.attnotnull THEN ' not null' ELSE '' END, \
                  coalesce(' default ' || pg_get_expr(d.adbin, d.adrelid), ''), \
                  CASE a.attidentity WHEN 'a' THEN ' identity always' WHEN 'd' THEN ' identity by default' \
                       ELSE '' END, \
                  CASE WHEN a.attgenerated IN ('s', 'v') THEN ' generated ' || a.attgenerated::text ELSE '' END) \
           FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid \
           JOIN pg_namespace n ON n.oid = c.relnamespace \
           LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
           WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') AND a.attnum > 0 AND NOT a.attisdropped \
           UNION ALL \
           SELECT format('constraint %s %s %s%s', c.relname, k.conname, pg_get_constraintdef(k.oid), \
                  CASE WHEN k.convalidated THEN '' ELSE ' not valid' END) \
           FROM pg_constraint k JOIN pg_class c ON c.oid = k.conrelid \
           JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'public' \
           UNION ALL \
           SELECT 'index ' || indexdef FROM pg_indexes WHERE schemaname = 'public' \
           UNION ALL \
           SELECT 'trigger ' || pg_get_triggerdef(t.oid) FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid \
           JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'public' AND NOT t.tgisinternal \
           UNION ALL \
           SELECT 'function ' || pg_get_functiondef(p.oid) FROM pg_proc p \
           JOIN pg_namespace n ON n.oid = p.pronamespace \
           WHERE n.nspname = 'public' AND p.prokind = 'f' AND NOT EXISTS ( \
               SELECT 1 FROM pg_depend e \
               WHERE e.classid = 'pg_proc'::regclass AND e.objid = p.oid AND e.deptype = 'e') \
           UNION ALL \
           SELECT format('extension %s %s', extname, extversion) FROM pg_extension \
         ) AS s (line) ORDER BY line",
    )
    .fetch_all(pool)
    .await
}
