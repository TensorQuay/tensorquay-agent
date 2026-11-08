//! GW-U-150: a task's viewers, bridges and Take over (ADR-0039 sections 5 and 6).
use super::{Full, Holder, MAX_VIEWERS, Screen, Screens, reset_over};
use crate::screen::relay::RESET_WAIT;
use std::time::{Duration, Instant};
use tqa_types::ConversationId;

fn task() -> Result<ConversationId, Box<dyn std::error::Error>> {
    Ok("0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6b".parse()?)
}

// TEST: GW-U-150
#[test]
fn gw_u_150_four_viewers_and_four_bridges_at_most_paired_in_order()
-> Result<(), Box<dyn std::error::Error>> {
    let mut screen = Screen::<&'static str>::default();
    let mut paired = Vec::new();
    for viewer in 1..=4 {
        paired.push(screen.open(viewer).map_err(|full| format!("{full:?}"))?.0);
    }
    assert_eq!(screen.open(5).err(), Some(Full::Viewers), "a fifth viewer");
    // Each held dial takes the oldest ask, and the socket reaches that viewer.
    for (index, bridge) in ["b1", "b2", "b3", "b4"].into_iter().enumerate() {
        let sender = screen
            .take_ask()
            .map_err(|full| format!("{full:?}"))?
            .ok_or("an ask")?;
        sender.send(bridge).map_err(|_| "paired")?;
        assert_eq!(paired[index].try_recv()?, bridge);
    }
    assert_eq!(
        screen.take_ask().err(),
        Some(Full::Bridges),
        "the fifth dial"
    );
    screen.bridge_closed();
    assert!(
        matches!(screen.take_ask(), Ok(None)),
        "a free bridge, but nobody asking: keep holding"
    );
    assert_eq!(MAX_VIEWERS, 4);
    Ok(())
}

// TEST: GW-U-150
#[test]
fn gw_u_150_a_viewer_gone_before_its_bridge_is_skipped() -> Result<(), Box<dyn std::error::Error>> {
    let mut screen = Screen::<u8>::default();
    let (gone, _) = screen.open(1).map_err(|full| format!("{full:?}"))?;
    let (mut waiting, _) = screen.open(2).map_err(|full| format!("{full:?}"))?;
    drop(gone);
    let sender = screen
        .take_ask()
        .map_err(|full| format!("{full:?}"))?
        .ok_or("an ask")?;
    sender.send(9).map_err(|_| "paired")?;
    assert_eq!(
        waiting.try_recv()?,
        9,
        "the dial went to the viewer still there"
    );
    Ok(())
}

// TEST: GW-U-150
#[test]
fn gw_u_150_one_viewer_holds_the_screen_and_its_close_hands_it_back()
-> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut screen = Screen::<u8>::default();
    let (_, control) = screen.open(1).map_err(|full| format!("{full:?}"))?;
    screen.open(2).map_err(|full| format!("{full:?}"))?;
    assert_eq!(*control.borrow(), Holder::Nobody);
    assert_eq!(screen.take_over(1, start), Ok(true));
    // Nothing to wait for (no run's reset pending): control is granted at once (GW-U-155 has the waiting).
    assert!(screen.grant(1, true));
    assert_eq!(
        screen.take_over(1, start),
        Ok(false),
        "again by the holder: nothing changes"
    );
    assert_eq!(
        screen.take_over(2, start),
        Err(Full::Taken),
        "a second controller"
    );
    assert_eq!(*control.borrow(), Holder::Viewer(1));
    assert!(screen.holds(1) && !screen.holds(2) && screen.taken_over());
    assert_eq!(
        screen.hand_back(2, start),
        None,
        "only the holder hands back"
    );
    // The holder's socket closes (a dropped network too): Take over ends, with its length for the notice.
    assert_eq!(
        screen.close(1, start + Duration::from_secs(95)),
        Some(Duration::from_secs(95))
    );
    assert_eq!(*control.borrow(), Holder::Nobody);
    assert!(!screen.taken_over());
    assert_eq!(
        screen.take_over(2, start),
        Ok(true),
        "free for another viewer now"
    );
    assert_eq!(
        screen.hand_back(2, start + Duration::from_secs(3)),
        Some(Duration::from_secs(3))
    );
    Ok(())
}

// TEST: GW-U-150
#[test]
fn gw_u_150_a_task_is_forgotten_once_nothing_is_open() -> Result<(), Box<dyn std::error::Error>> {
    let screens = Screens::<u8>::default();
    let id = task()?;
    screens
        .with(&id, |screen| screen.open(1).map(|_| ()))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    screens.with(&id, |screen| screen.take_over(1, Instant::now()));
    assert!(screens.taken_over(&id));
    screens.with(&id, |screen| screen.close(1, Instant::now()));
    assert!(!screens.taken_over(&id));
    assert!(
        screens.open.lock().map_err(|_| "lock")?.is_empty(),
        "nothing open: forgotten"
    );
    Ok(())
}

// TEST: GW-U-155
#[test]
fn gw_u_155_control_waits_for_the_run_end_reset_and_turns_stay_held_meanwhile()
-> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut screen = Screen::<u8>::default();
    let (_, control) = screen.open(1).map_err(|full| format!("{full:?}"))?;
    screen.open(2).map_err(|full| format!("{full:?}"))?;
    assert!(!screen.reset_pending(), "no run has ended");
    // A run is active when viewer 1 takes over: the relay notes the resets so far, and waits for one more.
    let before = screen.resets().borrow().done;
    assert_eq!(screen.take_over(1, start), Ok(true));
    // Claimed, so no queued turn starts and no other tab takes over; but no control yet, so no input passes.
    assert!(screen.taken_over());
    assert_eq!(screen.take_over(2, start), Err(Full::Taken));
    assert_eq!(*control.borrow(), Holder::Nobody);
    assert!(!screen.holds(1));
    // The runtime says the reset is over: the wait ends, and control is granted.
    let mut resets = screen.resets();
    screen.reset_done();
    assert!(!screen.reset_pending());
    assert!(resets.has_changed()?, "a waiting Take over is woken");
    assert_eq!(resets.borrow_and_update().done, before + 1);
    assert!(screen.grant(1, true));
    assert_eq!(*control.borrow(), Holder::Viewer(1));
    assert!(screen.holds(1));
    assert!(!screen.grant(1, true), "granted once");
    assert_eq!(
        screen.hand_back(1, start + Duration::from_secs(40)),
        Some(Duration::from_secs(40)),
        "held from the press"
    );
    Ok(())
}

// TEST: GW-U-155
#[test]
fn gw_u_155_a_run_that_just_ended_holds_control_until_its_reset()
-> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut screen = Screen::<u8>::default();
    let (_, control) = screen.open(1).map_err(|full| format!("{full:?}"))?;
    // A run ends while the panel is open; its reset is under way when the person presses Take over.
    screen.run_ended();
    assert_eq!(screen.take_over(1, start), Ok(true));
    assert!(screen.reset_pending());
    assert_eq!(*control.borrow(), Holder::Nobody);
    // A reset with nobody waiting just clears it.
    screen.reset_done();
    screen.reset_done();
    assert!(!screen.reset_pending());
    Ok(())
}

// TEST: GW-U-155
#[test]
fn gw_u_155_a_late_grant_says_so_and_a_closed_tab_is_never_granted()
-> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut screen = Screen::<u8>::default();
    let (_, control) = screen.open(1).map_err(|full| format!("{full:?}"))?;
    screen.open(2).map_err(|full| format!("{full:?}"))?;
    // The reset never came within the bound: control is granted anyway, marked late.
    assert_eq!(screen.take_over(1, start), Ok(true));
    assert!(screen.grant(1, false));
    assert_eq!(*control.borrow(), Holder::Late(1));
    assert!(screen.holds(1));
    assert_eq!(screen.hand_back(1, start), Some(Duration::ZERO));
    // A tab that closes while it waits ends its Take over; the wait's end then grants nothing.
    assert_eq!(screen.take_over(2, start), Ok(true));
    assert_eq!(
        screen.close(2, start + Duration::from_secs(2)),
        Some(Duration::from_secs(2))
    );
    assert!(!screen.grant(2, true));
    assert_eq!(*control.borrow(), Holder::Nobody);
    assert!(!screen.taken_over());
    Ok(())
}

// TEST: GW-U-156
#[test]
fn gw_u_156_control_after_a_reset_that_couldn_t_empty_the_profile_says_so()
-> Result<(), Box<dyn std::error::Error>> {
    let screens = Screens::<u8>::default();
    let id = task()?;
    let start = Instant::now();
    let (_, control) = screens
        .with(&id, |screen| screen.open(1))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    // The run's end reset came but couldn't empty the profile: control is granted, and says earlier sign-ins may remain.
    screens.reset_done(&id, false, true);
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, start)),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    assert_eq!(*control.borrow(), Holder::Unclean(1));
    // Clearing the browser's sign-ins while in control makes it clean for the next Take over.
    screens.signed_out(&id, true, true);
    assert_eq!(
        screens.with(&id, |screen| screen.hand_back(1, start)),
        Some(Some(Duration::ZERO))
    );
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, start)),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    assert_eq!(*control.borrow(), Holder::Viewer(1));
    assert_eq!(
        screens.with(&id, |screen| screen.hand_back(1, start)),
        Some(Some(Duration::ZERO))
    );
    // A failed sign-out sets it too; a later clean reset clears it.
    screens.signed_out(&id, false, true);
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, start)),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    assert_eq!(*control.borrow(), Holder::Unclean(1));
    assert_eq!(
        screens.with(&id, |screen| screen.hand_back(1, start)),
        Some(Some(Duration::ZERO))
    );
    screens.reset_done(&id, true, true);
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, start)),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    assert_eq!(*control.borrow(), Holder::Viewer(1));
    Ok(())
}

// TEST: GW-U-156
#[test]
fn gw_u_156_a_failed_reset_while_nobody_watches_is_still_said()
-> Result<(), Box<dyn std::error::Error>> {
    let screens = Screens::<u8>::default();
    let id = task()?;
    // The run ends with nobody watching (the usual case): its reset fails, and nothing of the screen is open.
    screens.reset_done(&id, false, true);
    assert!(
        screens.open.lock().map_err(|_| "lock")?.is_empty(),
        "no viewer entry is made for it"
    );
    // Later the person opens the screen and takes over: the warning is there.
    let (_, control) = screens
        .with(&id, |screen| screen.open(1))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, Instant::now())),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    assert_eq!(*control.borrow(), Holder::Unclean(1));
    Ok(())
}

// TEST: GW-U-156
#[test]
fn gw_u_156_a_late_grant_never_claims_to_know_the_reset() -> Result<(), Box<dyn std::error::Error>>
{
    let screens = Screens::<u8>::default();
    let id = task()?;
    screens.reset_done(&id, false, true);
    let (_, control) = screens
        .with(&id, |screen| screen.open(1))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, Instant::now())),
        Some(Ok(true))
    );
    // No reset came in time: late, whatever an earlier one said (its line covers it).
    assert_eq!(screens.grant(&id, 1, false), Some(true));
    assert_eq!(*control.borrow(), Holder::Late(1));
    Ok(())
}

// TEST: GW-U-156
/// The unclean mark is the profile's, and the profile lives in the sandbox: a new sandbox for the task (after an idle
/// retire, a hand-over or an environment change) starts clean, and a deleted task keeps no mark (the takeover pair's
/// review, P3).
#[test]
fn gw_u_156_a_new_sandbox_or_a_deleted_task_forgets_the_unclean_mark()
-> Result<(), Box<dyn std::error::Error>> {
    let screens = Screens::<u8>::default();
    let id = task()?;
    screens.reset_done(&id, false, true);
    screens.forget(&id);
    let (_, control) = screens
        .with(&id, |screen| screen.open(1))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, Instant::now())),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    assert_eq!(*control.borrow(), Holder::Viewer(1), "no false warning");
    screens.signed_out(&id, false, true);
    screens.forget(&id);
    assert!(
        screens.unclean.lock().map_err(|_| "lock")?.is_empty(),
        "nothing kept for a deleted task"
    );
    Ok(())
}

// TEST: GW-U-155
/// A late grant (the run's end reset unconfirmed within the wait) is taken back when that reset lands: the holder's
/// control turns into plain control after a clean reset, or says earlier sign-ins may remain after one that couldn't
/// empty the profile (the CU review, #20). Only a late grant changes; an on-time one, or no holder, gets nothing new.
#[test]
fn gw_u_155_a_late_grant_is_settled_when_the_reset_lands() -> Result<(), Box<dyn std::error::Error>>
{
    let start = Instant::now();
    for (ok, settled) in [(true, Holder::Viewer(1)), (false, Holder::Unclean(1))] {
        let screens = Screens::<u8>::default();
        let id = task()?;
        let (_, mut control) = screens
            .with(&id, |screen| screen.open(1))
            .ok_or("store")?
            .map_err(|full| format!("{full:?}"))?;
        screens.with(&id, Screen::run_ended);
        assert_eq!(
            screens.with(&id, |screen| screen.take_over(1, start)),
            Some(Ok(true))
        );
        assert_eq!(screens.grant(&id, 1, false), Some(true));
        assert_eq!(*control.borrow_and_update(), Holder::Late(1));
        screens.reset_done(&id, ok, true);
        assert!(control.has_changed()?, "the holder is told (ok {ok})");
        assert_eq!(*control.borrow_and_update(), settled);
        assert_eq!(
            screens.with(&id, |screen| screen.holds(1)),
            Some(true),
            "still in control"
        );
        // A second reset (another run's end) changes nothing more.
        screens.reset_done(&id, ok, true);
        assert!(!control.has_changed()?, "nothing new (ok {ok})");
    }
    // An on-time grant, then a reset: nothing new for anyone.
    let screens = Screens::<u8>::default();
    let id = task()?;
    let (_, mut control) = screens
        .with(&id, |screen| screen.open(1))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, start)),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    control.mark_unchanged();
    screens.reset_done(&id, true, true);
    assert!(!control.has_changed()?, "an on-time grant isn't sent again");
    // A late grant handed back before the reset lands: nobody holds, nothing is granted by the reset.
    assert_eq!(
        screens.with(&id, |screen| screen.hand_back(1, start)),
        Some(Some(Duration::ZERO))
    );
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, start)),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, false), Some(true));
    assert_eq!(
        screens.with(&id, |screen| screen.hand_back(1, start)),
        Some(Some(Duration::ZERO))
    );
    control.mark_unchanged();
    screens.reset_done(&id, true, true);
    assert!(!control.has_changed()?, "no holder: nothing to settle");
    assert_eq!(*control.borrow(), Holder::Nobody);
    Ok(())
}

// TEST: GW-U-196
/// A run that ends while nobody watches still holds the next Take over until its reset (`sec_059_a`, c16f): the screen
/// opened after the run's end starts with its reset due, and the wait ends when it comes.
#[tokio::test(start_paused = true)]
async fn gw_u_196_a_run_ended_while_nobody_watched_holds_control_until_its_reset()
-> Result<(), Box<dyn std::error::Error>> {
    let screens = Screens::<u8>::default();
    let id = task()?;
    screens.run_ended(&id);
    assert!(
        screens.open.lock().map_err(|_| "lock")?.is_empty(),
        "no viewer entry is made for it"
    );
    // The person opens the task and presses Take over before the runtime's reset is over.
    let (_, control) = screens
        .with(&id, |screen| screen.open(1))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    assert_eq!(
        screens.with(&id, |screen| screen.reset_pending()),
        Some(true)
    );
    let (resets, before) = screens
        .with(&id, |screen| {
            let resets = screen.resets();
            let before = resets.borrow().done;
            (resets, before)
        })
        .ok_or("store")?;
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, Instant::now())),
        Some(Ok(true))
    );
    let waiting = tokio::spawn(tokio::time::timeout(
        RESET_WAIT,
        reset_over(resets, before, false),
    ));
    tokio::task::yield_now().await;
    assert!(!waiting.is_finished(), "control waits for the reset");
    assert_eq!(*control.borrow(), Holder::Nobody);
    screens.reset_done(&id, true, true);
    assert_eq!(waiting.await?, Ok(true), "the reset ends the wait");
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    assert_eq!(*control.borrow(), Holder::Viewer(1));
    // Its reset came: the next screen opened starts with nothing due.
    assert_eq!(
        screens.with(&id, |screen| screen.close(1, Instant::now()).is_some()),
        Some(true)
    );
    assert!(screens.open.lock().map_err(|_| "lock")?.is_empty());
    screens
        .with(&id, |screen| screen.open(2))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    assert_eq!(
        screens.with(&id, |screen| screen.reset_pending()),
        Some(false)
    );
    Ok(())
}

// TEST: GW-U-196
/// The same with no reset ever coming: control is granted `Late` once `RESET_WAIT` has run out, never before.
#[tokio::test(start_paused = true)]
async fn gw_u_196_with_no_reset_control_is_late_after_the_wait()
-> Result<(), Box<dyn std::error::Error>> {
    let screens = Screens::<u8>::default();
    let id = task()?;
    screens.run_ended(&id);
    let (_, control) = screens
        .with(&id, |screen| screen.open(1))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    let (resets, before) = screens
        .with(&id, |screen| {
            let resets = screen.resets();
            let before = resets.borrow().done;
            (resets, before)
        })
        .ok_or("store")?;
    let pressed = tokio::time::Instant::now();
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, Instant::now())),
        Some(Ok(true))
    );
    let ready = matches!(
        tokio::time::timeout(RESET_WAIT, reset_over(resets, before, false)).await,
        Ok(true)
    );
    assert!(!ready, "no reset came");
    assert_eq!(pressed.elapsed(), RESET_WAIT, "it waited the whole bound");
    assert_eq!(screens.grant(&id, 1, ready), Some(true));
    assert_eq!(*control.borrow(), Holder::Late(1));
    Ok(())
}

// TEST: GW-U-196
/// The due mark is bounded: it goes with the task's sandbox (no reset comes from one that's gone; a task without a
/// desktop never sends one) and with a new sandbox or a deleted task (`forget`). A Take over then has nothing to wait for.
#[tokio::test(start_paused = true)]
async fn gw_u_196_the_due_mark_goes_with_the_sandbox_or_the_task()
-> Result<(), Box<dyn std::error::Error>> {
    let screens = Screens::<u8>::default();
    let id = task()?;
    screens.run_ended(&id);
    screens.sandbox_gone(&id);
    assert!(
        screens.due.lock().map_err(|_| "lock")?.is_empty(),
        "gone with the sandbox"
    );
    screens.run_ended(&id);
    screens.forget(&id);
    assert!(
        screens.due.lock().map_err(|_| "lock")?.is_empty(),
        "gone with a new sandbox or the task"
    );
    let (resets, before) = screens
        .with(&id, |screen| {
            screen
                .open(1)
                .map(|_| ())
                .map_err(|full| format!("{full:?}"))?;
            let resets = screen.resets();
            let before = resets.borrow().done;
            Ok::<_, String>((resets, before))
        })
        .ok_or("store")??;
    let pressed = tokio::time::Instant::now();
    assert!(
        reset_over(resets, before, false).await,
        "nothing to wait for"
    );
    assert_eq!(pressed.elapsed(), Duration::ZERO);
    Ok(())
}

// TEST: GW-U-200
/// Real-world use before 17.0 (17 Oct 2026): desktop tasks waited minutes for desktops whose tasks had finished. An
/// idle desktop may give way to a waiting one only while its browser's profile is known empty: a clean run-end reset or
/// sign-out came, and no Take over since (a hand sign-in serves the run after it, ADR-0039 section 6). Unknown is
/// never empty: a task this gateway hasn't seen reset (after a restart too) or a new sandbox keeps its desktop.
#[test]
fn gw_u_200_a_desktop_gives_way_only_with_its_profile_known_empty()
-> Result<(), Box<dyn std::error::Error>> {
    let screens = Screens::<u8>::default();
    let id = task()?;
    assert!(
        !screens.may_retire(&id),
        "no reset seen: a sign-in may be there"
    );
    screens.reset_done(&id, true, true);
    assert!(screens.may_retire(&id), "a clean reset emptied it");
    // A Take over: held now, then granted, then handed back; a sign-in may be there until the next reset.
    let _ = screens
        .with(&id, |screen| screen.open(1))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, Instant::now())),
        Some(Ok(true))
    );
    assert!(!screens.may_retire(&id), "held");
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    screens.with(&id, |screen| screen.close(1, Instant::now()));
    assert!(!screens.taken_over(&id));
    assert!(
        !screens.may_retire(&id),
        "handed back, its sign-in kept for the next run"
    );
    screens.reset_done(&id, true, true);
    assert!(screens.may_retire(&id), "the next run's end emptied it");
    // A reset that couldn't empty the profile, then Sign out everywhere that did.
    screens.reset_done(&id, false, true);
    assert!(!screens.may_retire(&id));
    screens.signed_out(&id, true, true);
    assert!(screens.may_retire(&id));
    // A new sandbox, or the old one gone: unknown again until a reset.
    screens.forget(&id);
    assert!(!screens.may_retire(&id));
    screens.reset_done(&id, true, true);
    screens.sandbox_gone(&id);
    assert!(!screens.may_retire(&id));
    Ok(())
}

// TEST: GW-U-203
/// A reset that says the browser didn't come back in time (16r's `sec_059_a`: a person typed into a desktop whose
/// browser wasn't there) leaves control not ready: a late grant stays late rather than turning plain, and a Take over
/// after it is granted not ready, until a reset says the browser is ready again.
#[test]
fn gw_u_203_a_reset_without_a_ready_browser_keeps_control_not_ready()
-> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let screens = Screens::<u8>::default();
    let id = task()?;
    let (_, mut control) = screens
        .with(&id, |screen| screen.open(1))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    screens.with(&id, Screen::run_ended);
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, start)),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, false), Some(true));
    assert_eq!(*control.borrow_and_update(), Holder::Late(1));
    // The profile was emptied, but the browser didn't come back in time: still not ready.
    screens.reset_done(&id, true, false);
    assert_eq!(
        *control.borrow_and_update(),
        Holder::Late(1),
        "a late grant stays late"
    );
    assert_eq!(
        screens.with(&id, |screen| screen.hand_back(1, start)),
        Some(Some(Duration::ZERO))
    );
    // Nothing pending now, yet the browser isn't known to be up: a Take over is granted not ready.
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, start)),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    assert_eq!(
        *control.borrow_and_update(),
        Holder::Late(1),
        "granted, not ready"
    );
    // The browser is said ready again (the next run's reset): the late grant settles, and the next one is plain.
    screens.reset_done(&id, true, true);
    assert_eq!(*control.borrow_and_update(), Holder::Viewer(1));
    assert_eq!(
        screens.with(&id, |screen| screen.hand_back(1, start)),
        Some(Some(Duration::ZERO))
    );
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, start)),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    assert_eq!(*control.borrow(), Holder::Viewer(1));
    Ok(())
}

// TEST: GW-U-204
/// Sign out everywhere closes the browser and starts it again, as a run's end does, so a Take over meanwhile waits for
/// its answer, and one after an answer whose browser wasn't back in time is granted not ready (the 16v integration's
/// `sec_059_a` review: the answer came before the browser, and control was plain over a desktop with none yet).
#[test]
fn gw_u_204_a_take_over_around_sign_out_everywhere_waits_and_is_not_plain_without_a_browser()
-> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let screens = Screens::<u8>::default();
    let id = task()?;
    let (_, mut control) = screens
        .with(&id, |screen| screen.open(1))
        .ok_or("store")?
        .map_err(|full| format!("{full:?}"))?;
    // Asked of the runtime: a reset is pending until its answer, so a Take over waits for it.
    screens.sign_out_asked(&id);
    assert_eq!(
        screens.with(&id, |screen| screen.reset_pending()),
        Some(true),
        "a Take over during a sign-out waits for its answer"
    );
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, start)),
        Some(Ok(true))
    );
    // The gateway's wait ran out first: a late grant.
    assert_eq!(screens.grant(&id, 1, false), Some(true));
    assert_eq!(*control.borrow_and_update(), Holder::Late(1));
    // Emptied, but the browser wasn't back in time: the late grant stays late, and nothing is pending any more.
    screens.signed_out(&id, true, false);
    assert_eq!(
        *control.borrow_and_update(),
        Holder::Late(1),
        "a late grant stays late"
    );
    assert_eq!(
        screens.with(&id, |screen| screen.reset_pending()),
        Some(false)
    );
    assert_eq!(
        screens.with(&id, |screen| screen.hand_back(1, start)),
        Some(Some(Duration::ZERO))
    );
    // A Take over now finds nothing pending, yet no browser known to be up: granted not ready.
    assert_eq!(
        screens.with(&id, |screen| screen.take_over(1, start)),
        Some(Ok(true))
    );
    assert_eq!(screens.grant(&id, 1, true), Some(true));
    assert_eq!(
        *control.borrow_and_update(),
        Holder::Late(1),
        "granted, not ready"
    );
    // A later answer with the browser ready settles it to plain control.
    screens.sign_out_asked(&id);
    screens.signed_out(&id, true, true);
    assert_eq!(*control.borrow_and_update(), Holder::Viewer(1));
    Ok(())
}
