//! GW-U-170: who must wait behind an older waiter on its workspace when it starts its own sandbox (`admit()`), the
//! rule that stopped a hand-over's holder from taking the workspace straight back (GW-I-251). GW-U-201: a holder whose
//! turn was delivered but whose run hasn't started has a turn in flight, so it isn't handed over.
use super::{Caps, admits_waiter, behind_older_waiter, block, turn_in_flight, yields_to_waiter};
use sqlx::PgPool;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn person(pool: &PgPool) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, password_hash, role) VALUES ($1, $2::text::citext, 'x', 'member')")
        .bind(id)
        .bind(format!("{id}@example.invalid"))
        .execute(pool)
        .await?;
    Ok(id)
}

async fn workspace(pool: &PgPool, owner: Uuid) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, owner_id, name) VALUES ($1, $2, 'w')")
        .bind(id)
        .bind(owner)
        .execute(pool)
        .await?;
    Ok(id)
}

async fn task(pool: &PgPool, owner: Uuid, workspace: Uuid) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO conversations (id, owner_id, title, model_route, backend, template, workspace_id) \
         VALUES ($1, $2, 'task', 'local', 'docker', 'workstation', $3)",
    )
    .bind(id)
    .bind(owner)
    .bind(workspace)
    .execute(pool)
    .await?;
    Ok(id)
}

/// A queued message turn of `conversation`, accepted `ago_seconds` ago.
async fn queued(
    pool: &PgPool,
    owner: Uuid,
    conversation: Uuid,
    ago_seconds: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO idempotency (key, user_id, route, response_status, response_body, conversation_id, queued_frame, \
         execution_status, created_at) VALUES ($1, $2, $3, 202, convert_to('{}', 'UTF8'), $4, convert_to('{}', 'UTF8'), 'queued', \
         CURRENT_TIMESTAMP - make_interval(secs => $5))",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(owner)
    .bind(format!("/api/v1/conversations/{conversation}/messages"))
    .bind(conversation)
    .bind(f64::from(ago_seconds))
    .execute(pool)
    .await?;
    Ok(())
}

async fn sandbox(pool: &PgPool, conversation: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO sandbox_instances (conversation_id, idle_seconds, max_lifetime_seconds) VALUES ($1, 600, 86400)")
        .bind(conversation)
        .execute(pool)
        .await?;
    Ok(())
}

async fn behind(
    pool: &PgPool,
    conversation: Uuid,
    per_user: i64,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut connection = pool.acquire().await?;
    Ok(behind_older_waiter(
        &mut connection,
        conversation,
        Caps {
            per_user,
            desktops: 4,
            taken: std::time::Duration::from_secs(30),
        },
    )
    .await?)
}

// TEST: GW-U-170
#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_u_170_a_start_with_no_queued_turn_waits_behind_an_older_waiter(
    pool: PgPool,
) -> TestResult {
    let owner = person(&pool).await?;
    let shared = workspace(&pool, owner).await?;
    let waiter = task(&pool, owner, shared).await?;
    let holder = task(&pool, owner, shared).await?;
    let files = task(&pool, owner, shared).await?;
    queued(&pool, owner, waiter, 60).await?;
    queued(&pool, owner, holder, 10).await?;
    // The holder's later turn waits behind the waiter's earlier one (the hand-over's holder, let back in before).
    assert!(
        behind(&pool, holder, 4).await?,
        "a later turn goes after the waiter"
    );
    // A start with no turn of its own (opening Files, a cancel) waits too: it would be retired as an idle holder.
    assert!(
        behind(&pool, files, 4).await?,
        "no queued turn: behind any waiter"
    );
    // The waiter itself is behind no one.
    assert!(
        !behind(&pool, waiter, 4).await?,
        "the oldest turn goes first"
    );
    // A waiter its owner's cap holds back is not one to wait for: it couldn't take the workspace.
    let other = person(&pool).await?;
    let capped_ws = workspace(&pool, owner).await?;
    let capped = task(&pool, other, capped_ws).await?;
    let elsewhere = task(&pool, other, workspace(&pool, other).await?).await?;
    sandbox(&pool, elsewhere).await?;
    let late = task(&pool, owner, capped_ws).await?;
    queued(&pool, other, capped, 60).await?;
    assert!(
        !behind(&pool, late, 1).await?,
        "a waiter at its owner's cap is not waited for"
    );
    assert!(behind(&pool, late, 2).await?, "below the cap it is");
    // A waiter that already has a sandbox is no waiter; the holder, its turn still queued with no sandbox, still is.
    sandbox(&pool, waiter).await?;
    assert!(
        behind(&pool, files, 4).await?,
        "the holder's queued turn is still waited for"
    );
    sandbox(&pool, holder).await?;
    assert!(
        !behind(&pool, files, 4).await?,
        "both started: nothing to wait for"
    );
    Ok(())
}

/// A Desktop environment's template, so a task bound to it declares `desktop`.
async fn desktop_template(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO templates (name, display_name, image, capabilities, is_default, version, last_set_by) \
         VALUES ('desktop', 'Desktop', $1, '{shell,browser,desktop}', false, 1, 'admin') ON CONFLICT (name) DO NOTHING",
    )
    .bind(format!("sha256:{}", "4".repeat(64)))
    .execute(pool)
    .await?;
    Ok(())
}

async fn on_desktop(pool: &PgPool, conversation: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE conversations SET template = 'desktop' WHERE id = $1")
        .bind(conversation)
        .execute(pool)
        .await?;
    Ok(())
}

/// `holder` holds `workspace`'s lease for an hour.
async fn lease(pool: &PgPool, workspace: Uuid, holder: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE workspaces SET lease_conversation_id = $2, lease_expires_at = CURRENT_TIMESTAMP + interval '1 hour' \
         WHERE id = $1",
    )
    .bind(workspace)
    .bind(holder)
    .execute(pool)
    .await?;
    Ok(())
}

// TEST: GW-U-172
#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_u_172_a_desktop_waiter_that_cant_start_is_no_waiter(pool: PgPool) -> TestResult {
    desktop_template(&pool).await?;
    let owner = person(&pool).await?;
    // One Desktop already live elsewhere, with `desktop.max_sessions` 1: a Desktop waiter can't start.
    let other = person(&pool).await?;
    let elsewhere = task(&pool, other, workspace(&pool, other).await?).await?;
    on_desktop(&pool, elsewhere).await?;
    sandbox(&pool, elsewhere).await?;
    let shared = workspace(&pool, owner).await?;
    let holder = task(&pool, owner, shared).await?;
    let waiter = task(&pool, owner, shared).await?;
    on_desktop(&pool, waiter).await?;
    sandbox(&pool, holder).await?;
    lease(&pool, shared, holder).await?;
    queued(&pool, owner, waiter, 60).await?;
    queued(&pool, owner, holder, 10).await?;
    let route = format!("/api/v1/conversations/{holder}/messages");
    let full = Caps {
        per_user: 4,
        desktops: 1,
        taken: std::time::Duration::from_secs(30),
    };
    let room = Caps {
        per_user: 4,
        desktops: 2,
        taken: std::time::Duration::from_secs(30),
    };
    let mut connection = pool.acquire().await?;
    // Desktops full: the waiter isn't one. The holder doesn't yield, isn't handed over, and a new start doesn't wait.
    assert!(!admits_waiter(&mut connection, waiter, Some(holder), full).await?);
    assert!(
        !yields_to_waiter(&mut connection, holder, &route, full).await?,
        "a plain holder frees no Desktop: its later turn goes"
    );
    let block_full = block(&mut connection, waiter, full)
        .await?
        .ok_or("the holder holds the lease")?;
    assert!(
        !block_full.handoff,
        "not handed over to a waiter that can't start"
    );
    let fresh = task(&pool, owner, shared).await?;
    assert!(
        !behind_older_waiter(&mut connection, fresh, full).await?,
        "nobody waits behind a waiter that can't start"
    );
    // Room for a second Desktop: the waiter is one again, in acceptance order.
    assert!(admits_waiter(&mut connection, waiter, Some(holder), room).await?);
    assert!(yields_to_waiter(&mut connection, holder, &route, room).await?);
    assert!(behind_older_waiter(&mut connection, fresh, room).await?);
    // A Desktop holder frees its own Desktop: with the other one gone, the waiter fits under a cap of one.
    sqlx::query("DELETE FROM sandbox_instances WHERE conversation_id = $1")
        .bind(elsewhere)
        .execute(&pool)
        .await?;
    on_desktop(&pool, holder).await?;
    assert!(admits_waiter(&mut connection, waiter, Some(holder), full).await?);
    assert!(
        !admits_waiter(&mut connection, waiter, None, full).await?,
        "without the holder's sandbox freed, its Desktop counts"
    );
    Ok(())
}

/// A turn of `conversation` delivered to its runtime `ago_seconds` ago.
async fn delivered(
    pool: &PgPool,
    owner: Uuid,
    conversation: Uuid,
    ago_seconds: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO idempotency (key, user_id, route, response_status, response_body, conversation_id, queued_frame, \
         execution_status, created_at, updated_at) VALUES ($1, $2, $3, 202, convert_to('{}', 'UTF8'), $4, \
         convert_to('{}', 'UTF8'), 'delivered', CURRENT_TIMESTAMP - make_interval(secs => $5 + 1), \
         CURRENT_TIMESTAMP - make_interval(secs => $5))",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(owner)
    .bind(format!("/api/v1/conversations/{conversation}/messages"))
    .bind(conversation)
    .bind(f64::from(ago_seconds))
    .execute(pool)
    .await?;
    Ok(())
}

/// A lifecycle event (`kind`) of `conversation`'s journal at `seq`, stored `ago_seconds` ago.
async fn lifecycle(
    pool: &PgPool,
    conversation: Uuid,
    (seq, kind): (i64, &str),
    ago_seconds: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO events (conversation_id, seq, run_id, ts, frame, lifecycle, created_at) VALUES ($1, $2, 'r-1', \
         CURRENT_TIMESTAMP, '{}'::jsonb, $3, CURRENT_TIMESTAMP - make_interval(secs => $4))",
    )
    .bind(conversation)
    .bind(seq)
    .bind(kind)
    .bind(f64::from(ago_seconds))
    .execute(pool)
    .await?;
    Ok(())
}

// TEST: GW-U-201
#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_u_201_a_holder_whose_turn_is_delivered_but_not_started_isnt_handed_over(
    pool: PgPool,
) -> TestResult {
    // 16o live V4 (18 Oct 2026): a my_files helper's first turn reached its runtime, and 70 ms later, before its run
    // started, its sandbox was handed over to the waiting helper; the turn was lost with it.
    let owner = person(&pool).await?;
    let shared = workspace(&pool, owner).await?;
    let holder = task(&pool, owner, shared).await?;
    let waiter = task(&pool, owner, shared).await?;
    sandbox(&pool, holder).await?;
    lease(&pool, shared, holder).await?;
    queued(&pool, owner, waiter, 60).await?;
    // An earlier run ended; then the holder's next turn was delivered, and its run hasn't started.
    lifecycle(&pool, holder, (1, "run.completed"), 60).await?;
    delivered(&pool, owner, holder, 10).await?;
    let caps = Caps {
        per_user: 4,
        desktops: 4,
        taken: std::time::Duration::from_secs(30),
    };
    let mut connection = pool.acquire().await?;
    let waiting = block(&mut connection, waiter, caps)
        .await?
        .ok_or("the holder holds the lease")?;
    assert!(
        !waiting.handoff,
        "a turn delivered and not yet started is in flight"
    );
    assert!(turn_in_flight(&mut connection, holder, caps.taken).await?);
    assert!(
        !turn_in_flight(&mut connection, waiter, caps.taken).await?,
        "a queued turn alone isn't in flight"
    );
    // Its run starts and ends: nothing in flight, so the waiter gets the workspace.
    lifecycle(&pool, holder, (2, "run.started"), 5).await?;
    lifecycle(&pool, holder, (3, "run.completed"), 1).await?;
    let ended = block(&mut connection, waiter, caps)
        .await?
        .ok_or("the holder holds the lease")?;
    assert!(ended.handoff, "the run that took the turn has ended");
    assert!(!turn_in_flight(&mut connection, holder, caps.taken).await?);
    sqlx::query("UPDATE idempotency SET execution_status = 'sending' WHERE conversation_id = $1")
        .bind(holder)
        .execute(&pool)
        .await?;
    assert!(
        turn_in_flight(&mut connection, holder, caps.taken).await?,
        "a turn being sent is in flight"
    );
    Ok(())
}
