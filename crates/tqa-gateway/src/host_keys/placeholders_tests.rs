//! GW-I-165, the store (ADR-0037 §3): a run's placeholders are minted per key, kept only as hashes, bound to the run
//! its sandbox starts, kept while it resumes, ended with it and kept a day, and gone with an undelivered turn, a newer
//! turn, the sandbox's credential or the task.
use super::{
    Bound, bind_run, drop_delivery, end_run, end_sandbox, end_tasks, find, mint, sweep_batch,
};
use crate::host_keys::run_keys::RunKey;
use secrecy::ExposeSecret;
use sqlx::{PgPool, Postgres, pool::PoolConnection};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn fixture(pool: &PgPool) -> Result<(Uuid, Uuid, RunKey), sqlx::Error> {
    let user = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, password_hash, role) VALUES ($1, 'a@example.invalid', 'x', 'member')")
        .bind(user)
        .execute(pool)
        .await?;
    let workspace = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, owner_id, name) VALUES ($1, $2, 'w')")
        .bind(workspace)
        .bind(user)
        .execute(pool)
        .await?;
    let conversation = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO conversations (id, owner_id, title, model_route, backend, template, workspace_id) \
         VALUES ($1, $2, 'task', 'local', 'docker', 'workstation', $3)",
    )
    .bind(conversation)
    .bind(user)
    .bind(workspace)
    .execute(pool)
    .await?;
    let key = RunKey {
        id: Uuid::now_v7(),
        name: "GITHUB_WORK".into(),
        host: "github.com".into(),
        port: 443,
        path_prefix: Some("/acme".into()),
        scheme: "basic".into(),
        user_name: Some("x-access-token".into()),
        shared: false,
    };
    insert(pool, user, &key).await?;
    Ok((user, conversation, key))
}

async fn insert(pool: &PgPool, user: Uuid, key: &RunKey) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO host_keys (id, owner_id, name, host, port, path_prefix, scheme, user_name, secret_ref) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(key.id)
    .bind(user)
    .bind(&key.name)
    .bind(&key.host)
    .bind(i32::from(key.port))
    .bind(&key.path_prefix)
    .bind(&key.scheme)
    .bind(&key.user_name)
    // Each key its own store name, from its id.
    .bind(format!("secrets/host-{:0>64}", key.id.simple().to_string()))
    .execute(pool)
    .await?;
    Ok(())
}

/// A member's task with one key, and the store's connection.
struct Store {
    pool: PgPool,
    connection: PoolConnection<Postgres>,
    sender: Uuid,
    conversation: Uuid,
    key: RunKey,
}

impl Store {
    async fn new(pool: PgPool) -> Result<Self, Box<dyn std::error::Error>> {
        let (sender, conversation, key) = fixture(&pool).await?;
        let connection = pool.acquire().await?;
        Ok(Self {
            pool,
            connection,
            sender,
            conversation,
            key,
        })
    }

    /// One placeholder for the key, for `sandbox`, as turn `delivery` delivers it.
    async fn mint(
        &mut self,
        sandbox: &str,
        delivery: Uuid,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let key = self.key.clone();
        let minted = mint(
            &mut self.connection,
            (self.conversation, sandbox, self.sender),
            delivery,
            &[key],
        )
        .await?;
        Ok(minted[0].1.expose_secret().to_owned())
    }

    async fn find(
        &mut self,
        placeholder: &str,
    ) -> Result<Option<Bound>, Box<dyn std::error::Error>> {
        Ok(find(&mut self.connection, placeholder).await?)
    }

    /// What a placeholder minted for the key in `sandbox` is bound to, before a run starts.
    fn unbound(&self, sandbox: &str) -> Bound {
        let key = self.key.clone();
        Bound {
            key_id: Some(key.id),
            secret_ref: Some(format!("secrets/host-{:0>64}", key.id.simple().to_string())),
            conversation_id: self.conversation,
            sandbox_id: sandbox.into(),
            run_id: None,
            ended: false,
            sender_id: self.sender,
            name: key.name,
            host: key.host,
            port: key.port,
            path_prefix: key.path_prefix,
            scheme: key.scheme,
            user_name: key.user_name,
        }
    }

    async fn bind(&mut self, run: &str) -> TestResult {
        Ok(bind_run(&mut self.connection, self.conversation, run).await?)
    }

    async fn end(&mut self, run: &str) -> TestResult {
        Ok(end_run(&mut self.connection, self.conversation, run).await?)
    }
}

// TEST: GW-I-165
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_165_a_placeholder_is_minted_per_key_and_stored_only_as_its_hash(
    pool: PgPool,
) -> TestResult {
    let mut store = Store::new(pool).await?;
    let mut other = store.key.clone();
    other.id = Uuid::now_v7();
    other.name = "OTHER".into();
    insert(&store.pool, store.sender, &other).await?;
    let both = [store.key.clone(), other];
    let minted = mint(
        &mut store.connection,
        (store.conversation, "sandbox-1", store.sender),
        Uuid::now_v7(),
        &both,
    )
    .await?;
    // One per key, `tqa_ph_` and 32 random bytes in base64url, each different.
    assert_eq!(minted.len(), 2);
    for (_, placeholder) in &minted {
        let text = placeholder.expose_secret();
        assert!(
            text.starts_with("tqa_ph_") && text.len() == 7 + 43,
            "{}",
            text.len()
        );
        assert!(
            text[7..]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
    }
    assert_ne!(minted[0].1.expose_secret(), minted[1].1.expose_secret());
    let placeholder = minted[0].1.expose_secret().to_owned();
    let stored: String = sqlx::query_scalar(
        "SELECT string_agg(row_to_json(p)::text, '') FROM host_key_placeholders p",
    )
    .fetch_one(&store.pool)
    .await?;
    assert!(
        !stored.contains(&placeholder[7..]),
        "only its hash is stored"
    );
    // Bound to the task, the sandbox, the sender and where the key goes; no run until the runtime starts one.
    assert_eq!(
        store.find(&placeholder).await?,
        Some(store.unbound("sandbox-1"))
    );
    assert_eq!(store.find("tqa_ph_unknown").await?, None);
    Ok(())
}

// TEST: GW-I-165
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_165_the_run_that_starts_binds_it_and_no_later_run_can(pool: PgPool) -> TestResult {
    let mut store = Store::new(pool).await?;
    let placeholder = store.mint("sandbox-1", Uuid::now_v7()).await?;
    // The run's start binds it; a resume (a runtime restart) keeps it; a later run can't take it over.
    store.bind("run-1").await?;
    store.bind("run-1").await?;
    store.bind("run-2").await?;
    assert_eq!(
        store
            .find(&placeholder)
            .await?
            .ok_or("bound")?
            .run_id
            .as_deref(),
        Some("run-1")
    );
    Ok(())
}

// TEST: GW-I-165
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_165_a_deleted_key_leaves_its_placeholder_without_a_key(pool: PgPool) -> TestResult {
    let mut store = Store::new(pool).await?;
    let placeholder = store.mint("sandbox-1", Uuid::now_v7()).await?;
    store.bind("run-1").await?;
    // A use is then `revoked`, not `absent`.
    sqlx::query("DELETE FROM host_keys WHERE id=$1")
        .bind(store.key.id)
        .execute(&store.pool)
        .await?;
    let kept = store.find(&placeholder).await?.ok_or("kept")?;
    assert_eq!((kept.key_id, kept.secret_ref), (None, None));
    Ok(())
}

// TEST: GW-I-165
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_165_a_runs_end_ends_its_placeholders_which_are_swept_a_day_later(
    pool: PgPool,
) -> TestResult {
    let mut store = Store::new(pool).await?;
    let first = store.mint("sandbox-1", Uuid::now_v7()).await?;
    store.bind("run-1").await?;
    let second = store.mint("sandbox-1", Uuid::now_v7()).await?;
    store.bind("run-2").await?;
    // Only its own, kept a day so a later use reads expired.
    store.end("run-1").await?;
    assert!(store.find(&first).await?.ok_or("kept")?.ended);
    assert!(!store.find(&second).await?.ok_or("going")?.ended);
    let recent = store.mint("sandbox-1", Uuid::now_v7()).await?;
    store.bind("run-3").await?;
    store.end("run-3").await?;
    // A day later the sweep removes them in bounded batches; ended ones within the day and a going run's stay.
    sqlx::query("UPDATE host_key_placeholders SET ended_at = ended_at - interval '25 hours' WHERE run_id = 'run-1'")
        .execute(&store.pool)
        .await?;
    let other = store.mint("sandbox-1", Uuid::now_v7()).await?;
    store.bind("run-4").await?;
    store.end("run-4").await?;
    sqlx::query("UPDATE host_key_placeholders SET ended_at = ended_at - interval '25 hours' WHERE run_id = 'run-4'")
        .execute(&store.pool)
        .await?;
    assert_eq!(sweep_batch(&mut store.connection, 1).await?, 1);
    assert_eq!(sweep_batch(&mut store.connection, 1).await?, 1);
    assert_eq!(sweep_batch(&mut store.connection, 1).await?, 0);
    for gone in [&first, &other] {
        assert_eq!(store.find(gone).await?, None);
    }
    assert!(store.find(&recent).await?.is_some(), "ended within the day");
    assert!(store.find(&second).await?.is_some(), "a going run's");
    Ok(())
}

// TEST: GW-I-165
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_165_an_untaken_turn_drops_its_placeholders_and_a_new_turn_replaces_them(
    pool: PgPool,
) -> TestResult {
    let mut store = Store::new(pool).await?;
    // A turn the runtime never took drops its placeholders.
    let untaken = Uuid::now_v7();
    let dropped = store.mint("sandbox-1", untaken).await?;
    drop_delivery(&mut store.connection, untaken).await?;
    assert_eq!(store.find(&dropped).await?, None);
    // A started run's are not a delivery's to drop.
    let taken = Uuid::now_v7();
    let started = store.mint("sandbox-1", taken).await?;
    store.bind("run-1").await?;
    drop_delivery(&mut store.connection, taken).await?;
    assert!(
        store.find(&started).await?.is_some(),
        "a started run's placeholders stay"
    );
    // A new turn's placeholders replace an earlier turn's that no run took (a busy refusal, a lost link).
    let refused = store.mint("sandbox-1", Uuid::now_v7()).await?;
    let next = store.mint("sandbox-1", Uuid::now_v7()).await?;
    assert_eq!(store.find(&refused).await?, None);
    assert!(store.find(&next).await?.is_some());
    assert!(
        store.find(&started).await?.is_some(),
        "a going run's are never replaced"
    );
    Ok(())
}

// TEST: GW-I-165
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_165_a_sandboxs_credential_or_its_tasks_deletion_ends_them(
    pool: PgPool,
) -> TestResult {
    let mut store = Store::new(pool).await?;
    let bound = store.mint("sandbox-1", Uuid::now_v7()).await?;
    store.bind("run-1").await?;
    let untaken = store.mint("sandbox-1", Uuid::now_v7()).await?;
    // The credential revoked or replaced: its untaken placeholders go; a run's stay until it ends, and the proxy
    // refuses them anywhere else (foreign_sandbox).
    end_sandbox(&mut store.connection, "sandbox-2").await?;
    assert!(store.find(&untaken).await?.is_some(), "another sandbox's");
    end_sandbox(&mut store.connection, "sandbox-1").await?;
    assert_eq!(store.find(&untaken).await?, None);
    assert!(store.find(&bound).await?.is_some());
    // A deleted task's go with it.
    end_tasks(&mut store.connection, &[store.conversation]).await?;
    assert_eq!(store.find(&bound).await?, None);
    Ok(())
}
