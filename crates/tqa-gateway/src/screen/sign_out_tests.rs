//! GW-U-148: a task's sign-outs share one ask to the runtime.
use super::{Turn, Waiters};
use tqa_types::{ConversationId, SignOutFailure};

fn task() -> Result<ConversationId, Box<dyn std::error::Error>> {
    Ok("0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6b".parse()?)
}

// TEST: GW-U-148
#[test]
fn gw_u_148_a_second_sign_out_while_one_waits_joins_it() -> Result<(), Box<dyn std::error::Error>> {
    let waiters = Waiters::default();
    let id = task()?;
    let Turn::Ask(mut first) = waiters.join_or_start(&id) else {
        return Err("the first sign-out asks the runtime".into());
    };
    // Another tab, a new key: it joins, and asks nothing.
    assert!(matches!(waiters.join_or_start(&id), Turn::Joined));
    // The runtime's one answer reaches the first, not lost to the second.
    waiters.answer(&id, Err(SignOutFailure::ProfileInUse));
    assert_eq!(first.try_recv()?, Err(SignOutFailure::ProfileInUse));
    // Answered but not yet recorded: a third still joins, so the outcome is recorded once.
    assert!(matches!(waiters.join_or_start(&id), Turn::Joined));
    waiters.finish(&id);
    // Recorded: the next sign-out asks again, and an answer for nobody is ignored.
    assert!(matches!(waiters.join_or_start(&id), Turn::Ask(_)));
    waiters.finish(&id);
    waiters.answer(&id, Ok(()));
    Ok(())
}

// TEST: GW-U-148
#[test]
fn gw_u_148_each_task_has_its_own_sign_out() -> Result<(), Box<dyn std::error::Error>> {
    let waiters = Waiters::default();
    let first: ConversationId = task()?;
    let second: ConversationId = "0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6c".parse()?;
    let Turn::Ask(mut one) = waiters.join_or_start(&first) else {
        return Err("asks".into());
    };
    let Turn::Ask(mut two) = waiters.join_or_start(&second) else {
        return Err("another task asks too".into());
    };
    waiters.answer(&second, Ok(()));
    assert_eq!(two.try_recv()?, Ok(()));
    assert!(
        one.try_recv().is_err(),
        "the other task's answer isn't this one's"
    );
    Ok(())
}
