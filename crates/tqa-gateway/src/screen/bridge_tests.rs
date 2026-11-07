//! GW-U-150: a granted bridge frees its count on every path that never pairs (ADR-0039 section 5).
use super::Granted;
use crate::screen::viewers::{Full, MAX_VIEWERS, Screen, Screens};
use std::sync::LazyLock;
use tqa_types::ConversationId;

static GRANTED: LazyLock<Screens<u8>> = LazyLock::new(Screens::default);

/// A grant, and the viewer's end it would reach.
type Grant = (Granted<u8>, tokio::sync::oneshot::Receiver<u8>);

fn task(last: u8) -> Result<ConversationId, Box<dyn std::error::Error>> {
    Ok(format!("0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a{last:02x}").parse()?)
}

/// Opens four viewers and grants each a bridge; the fifth grant is refused.
fn four_granted(conversation: &ConversationId) -> Result<Vec<Grant>, Box<dyn std::error::Error>> {
    let mut granted = Vec::new();
    for viewer in 1..=MAX_VIEWERS as u64 {
        let (paired, _) = GRANTED
            .with(conversation, |screen| screen.open(viewer))
            .ok_or("screens")?
            .map_err(|full| format!("{full:?}"))?;
        let sender = GRANTED
            .with(conversation, Screen::take_ask)
            .ok_or("screens")?
            .map_err(|full| format!("{full:?}"))?
            .ok_or("an ask")?;
        granted.push((Granted::new(&GRANTED, conversation.clone(), sender), paired));
    }
    Ok(granted)
}

fn full(conversation: &ConversationId) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(matches!(
        GRANTED
            .with(conversation, Screen::take_ask)
            .ok_or("screens")?,
        Err(Full::Bridges)
    ))
}

// TEST: GW-U-150
#[test]
fn gw_u_150_a_bridge_whose_upgrade_never_completes_frees_its_count()
-> Result<(), Box<dyn std::error::Error>> {
    let conversation = task(1)?;
    let mut granted = four_granted(&conversation)?;
    assert!(full(&conversation)?, "four bridges counted");
    // The runtime's dial dropped after the 101: the upgrade's callback is dropped unrun, and the grant with it.
    drop(granted.pop());
    assert!(
        !full(&conversation)?,
        "the dropped grant's count is free again"
    );
    Ok(())
}

// TEST: GW-U-150
#[test]
fn gw_u_150_a_bridge_whose_viewer_left_frees_its_count_and_a_paired_one_keeps_it()
-> Result<(), Box<dyn std::error::Error>> {
    let conversation = task(2)?;
    let mut granted = four_granted(&conversation)?;
    // Paired: the socket reaches its viewer, and the bridge stays counted until the relay ends it.
    let (grant, mut paired) = granted.pop().ok_or("a grant")?;
    grant.hand_over(7);
    assert_eq!(paired.try_recv()?, 7);
    assert!(full(&conversation)?, "a paired bridge stays counted");
    // The viewer left in between: the socket has nobody to reach, and the count is freed.
    let (grant, paired) = granted.pop().ok_or("a grant")?;
    drop(paired);
    grant.hand_over(8);
    assert!(
        !full(&conversation)?,
        "the unpaired grant's count is free again"
    );
    Ok(())
}
