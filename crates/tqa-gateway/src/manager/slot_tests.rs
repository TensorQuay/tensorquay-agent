//! GW-U-171: which slots a full slot table may drop (GW-I-251's stall). Only one that is idle (no sandbox, no worker,
//! no teardown whose sandbox may still exist) and that the table alone holds; a switch, refusal or busy turn left on a
//! slot without a sandbox doesn't keep it.
use super::{Slot, evictable};
use std::sync::{Arc, atomic::Ordering};

// TEST: GW-U-171
#[test]
fn gw_u_171_only_an_idle_slot_the_table_alone_holds_is_evictable() -> Result<(), &'static str> {
    let empty = Arc::new(Slot::empty());
    assert!(evictable(&empty), "an empty slot serves nothing");
    let held = Arc::clone(&empty);
    assert!(
        !evictable(&empty),
        "held by a worker, a schedule, a start or a teardown"
    );
    drop(held);
    assert!(evictable(&empty));
    for why in [
        "a worker runs",
        "a teardown is unconfirmed",
        "a teardown retry runs",
    ] {
        let slot = Arc::new(Slot::empty());
        let flag = match why {
            "a worker runs" => &slot.running,
            "a teardown is unconfirmed" => &slot.unconfirmed,
            _ => &slot.retrying,
        };
        flag.store(true, Ordering::Release);
        assert!(!evictable(&slot), "kept: {why}");
    }
    // A turn in flight keeps it too, though a sandboxless slot can't have one: a delivery needs a sandbox, and the rows
    // stay `sending` in the store, where `reclaim` puts them back to `queued` whatever is in memory.
    let in_flight = Arc::new(Slot::empty());
    in_flight
        .turns
        .hold_for_test(vec![("key".into(), uuid::Uuid::nil())]);
    assert!(!evictable(&in_flight), "kept: a turn in flight");
    let with_sandbox = Arc::new(Slot::empty());
    *with_sandbox.handle.lock().map_err(|_| "handle lock")? = Some(tqa_sandbox::SandboxHandle {
        kind: tqa_sandbox::BackendKind::Docker,
        id: "sandbox-1".into(),
        conversation_id: "0199a0f0-0000-7000-8000-000000000001"
            .parse()
            .map_err(|_| "id")?,
        created_at: std::time::SystemTime::UNIX_EPOCH,
    });
    assert!(!evictable(&with_sandbox), "kept: it has a sandbox");
    // Flags that only mean something beside a sandbox don't pin a slot without one.
    let stale = Arc::new(Slot::empty());
    stale.switch.store(true, Ordering::Release);
    stale.busy.store(true, Ordering::Release);
    stale.refused.store(1, Ordering::Release);
    assert!(
        evictable(&stale),
        "a switch, refusal or busy turn without a sandbox serves nothing"
    );
    Ok(())
}
