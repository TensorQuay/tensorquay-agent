use super::{MAX_HOP, Waiting, key, message_hop, named, sentence, wake_hop};
use uuid::Uuid;

// TEST: GW-U-227
#[test]
fn gw_u_227_a_wake_is_keyed_by_the_members_place_and_its_failures() {
    let (channel, member) = (Uuid::from_u128(1), Uuid::from_u128(2));
    assert_eq!(
        key(channel, member, 7, None),
        format!("channel-wake:{}:{}:7", channel.simple(), member.simple())
    );
    assert_eq!(
        key(channel, member, 7, Some(3)),
        format!(
            "channel-wake:{}:{}:7:failed-3",
            channel.simple(),
            member.simple()
        )
    );
    // Another channel, another member or another place is another wake; the same is the same one.
    assert_ne!(
        key(channel, member, 7, None),
        key(Uuid::from_u128(3), member, 7, None)
    );
    assert_ne!(key(channel, member, 7, None), key(channel, member, 8, None));
    assert_eq!(key(channel, member, 7, None), key(channel, member, 7, None));
    // An idempotency key is at most 128 bytes.
    assert!(key(Uuid::max(), Uuid::max(), i64::MAX, Some(i64::MAX)).len() <= 128);
}

// TEST: GW-U-228
#[test]
fn gw_u_228_the_wake_names_kinds_and_seqs_and_the_call_that_reads_them() {
    // It ends with the manners (GW-U-236 pins their words; decided on 20 Nov 2026).
    use super::sentence::MANNERS;
    let channel = Uuid::from_u128(9);
    assert_eq!(
        sentence(
            channel,
            11,
            &[
                Waiting::Proposal(12),
                Waiting::Task(13),
                Waiting::Named(14),
                Waiting::Named(15),
                Waiting::Reply(16),
                Waiting::Decision(17),
                Waiting::Report(18),
            ]
        ),
        format!(
            "New in channel {channel} since message 11: a proposal for your vote (message 12); a to-do open for \
             claims (message 13); 2 posts naming you (messages 14, 15); a reply to your post (message 16); the \
             decision on your proposal (message 17); a report on your to-do (message 18). Read them with \
             channel_read (channel {channel}, since 11). {MANNERS}"
        )
    );
    assert_eq!(
        sentence(channel, 0, &[Waiting::Proposal(3), Waiting::Proposal(5)]),
        format!(
            "New in channel {channel} since message 0: 2 proposals for your vote (messages 3, 5). Read them with \
             channel_read (channel {channel}, since 0). {MANNERS}"
        )
    );
}

// TEST: GW-U-236
// Every wake ends with the channel's manners, fixed words (decided on 20 Nov 2026, replacing C-4's one sentence): the
// channel can't see the task, so an answer is posted, as a reply; a post to everyone is answered by whoever can help;
// silence only when plainly not for it or already answered; short posts, no thanks; the turn ends after posting.
#[test]
fn gw_u_236_every_wake_ends_with_the_channels_manners() {
    let channel = Uuid::from_u128(9);
    let manners = "The people and agents in this channel don't see your task: whatever they should read, post it \
                   with channel_post, as a reply (parent) to the message you answer. A post to the whole channel is \
                   for whoever can help: read what others already answered, then answer it if nobody has, or add \
                   only what is missing. Take on one to-do at a time: claim it with task_claim before working on it, \
                   report it with channel_post when done, then claim another if any is left; leave to-dos others have \
                   claimed to them. Stay silent only when a post is plainly not for you or already answered. \
                   Keep posts short, with no thanks or acknowledgements. End your turn when you have posted or have \
                   nothing to add.";
    for waiting in [
        vec![Waiting::Channel(4)],
        vec![Waiting::Named(4)],
        vec![Waiting::Proposal(4), Waiting::Reply(5)],
    ] {
        let said = sentence(channel, 3, &waiting);
        assert!(said.ends_with(manners), "{said}");
        assert!(
            said.contains(&format!(
                " Read them with channel_read (channel {channel}, since 3). The people"
            )),
            "{said}"
        );
        assert!(!said.contains("waiting for more"), "{said}");
    }
}

// TEST: GW-U-229
#[test]
fn gw_u_229_a_post_names_members_by_task_title_longest_first_without_case() {
    let (pricing, analyst, checker) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
    let members = [
        (analyst, "Analyst"),
        (pricing, "Pricing analyst"),
        (checker, "Data checker"),
    ];
    // The longest name wins where two start at the same "@".
    assert_eq!(
        named("@pricing analyst, please look", &members),
        vec![pricing]
    );
    assert_eq!(
        named("@Analyst and @DATA CHECKER: both", &members),
        vec![analyst, checker]
    );
    // Each once, in the order first named.
    assert_eq!(
        named("@Data checker then @data checker", &members),
        vec![checker]
    );
    // No "@", or a name that isn't a member's, names no one; an e-mail address isn't a mention.
    assert!(named("Analyst, please look", &members).is_empty());
    assert!(named("@Nobody here", &members).is_empty());
    assert!(named("write to analyst@example.org", &members).is_empty());
    // A name must end where a word ends: "@Analysts" isn't "@Analyst".
    assert!(named("@Analysts", &members).is_empty());
}

// TEST: GW-U-230
#[test]
fn gw_u_230_hops_start_at_a_persons_post_and_stop_at_four() {
    assert_eq!(message_hop(true, None), 0);
    assert_eq!(message_hop(true, Some(2)), 0);
    assert_eq!(message_hop(false, None), 1);
    assert_eq!(message_hop(false, Some(0)), 1);
    assert_eq!(message_hop(false, Some(3)), 4);
    // The smallest unread hop wakes: a person's post in the burst starts a fresh chain.
    assert_eq!(wake_hop(&[3, 0, 2]), Some(0));
    assert_eq!(wake_hop(&[2, 3]), Some(2));
    // At the bound nothing wakes; below it, it does.
    assert_eq!(wake_hop(&[MAX_HOP]), None);
    assert_eq!(wake_hop(&[MAX_HOP, MAX_HOP + 1]), None);
    assert_eq!(wake_hop(&[MAX_HOP, MAX_HOP - 1]), Some(MAX_HOP - 1));
    assert_eq!(wake_hop(&[]), None);
}

// TEST: GW-U-234
// SEC-068 (review of 20 Nov 2026): the wake says how many files wait and from which messages, never a path (a channel's
// or a file's name is someone's words); channel_read's result lists the paths.
#[test]
fn gw_u_234_the_wake_counts_the_files_and_names_their_messages_only() {
    assert_eq!(super::files_sentence(&[]), "");
    assert_eq!(
        super::files_sentence(&[12]),
        " 1 file waits in your workspace, from message 12; channel_read lists its path."
    );
    assert_eq!(
        super::files_sentence(&[12, 12, 14]),
        " 3 files wait in your workspace, from messages 12 and 14; channel_read lists their paths."
    );
    assert_eq!(
        super::files_sentence(&[12, 14, 16]),
        " 3 files wait in your workspace, from messages 12, 14 and 16; channel_read lists their paths."
    );
}

// TEST: GW-U-237
// C-13 (21 Nov 2026): the wake lists to-dos as open for claims, never as work it may take on (one agent took all
// three); the manners say to take one at a time: claim, report, then the next, and leave others' to-dos to them.
#[test]
fn gw_u_237_the_wake_shares_out_to_dos_one_at_a_time() {
    let channel = Uuid::from_u128(9);
    let one = sentence(channel, 3, &[Waiting::Task(4)]);
    assert!(
        one.contains(": a to-do open for claims (message 4)."),
        "{one}"
    );
    let three = sentence(
        channel,
        3,
        &[Waiting::Task(4), Waiting::Task(5), Waiting::Task(6)],
    );
    assert!(
        three.contains(": 3 to-dos open for claims (messages 4, 5, 6)."),
        "{three}"
    );
    assert!(!three.contains("you may take on"), "{three}");
    assert!(
        three.contains(
            "Take on one to-do at a time: claim it with task_claim before working on it, report it with \
             channel_post when done, then claim another if any is left; leave to-dos others have claimed to them."
        ),
        "{three}"
    );
}

// TEST: GW-U-239
// A woken run that failed is said in its channel with what the page needs to word it as the task page does (decided on
// 21 Nov 2026, the live check of a used-up channel): its code and, when the gateway refused it, that refusal
// ("gateway.channel_budget 409"); any other failure detail, which may be the task's own words, never reaches the channel.
#[test]
fn gw_u_239_a_failed_runs_notice_carries_the_gateways_refusal_and_nothing_else()
-> Result<(), Box<dyn std::error::Error>> {
    use super::Ending;
    use tqa_types::{ErrorCode, Event, ServerFrame};
    let failed =
        |reason: &str, code: ErrorCode| -> Result<ServerFrame, Box<dyn std::error::Error>> {
            Ok(ServerFrame::Event {
                seq: 1.into(),
                run_id: None,
                step: None,
                ts: "2026-11-21T00:00:00Z".into(),
                event: Event::RunFailed {
                    run_id: "r-1".parse()?,
                    reason: reason.into(),
                    code,
                    provider_message: None,
                },
            })
        };
    assert_eq!(
        Ending::of(
            "run.failed",
            &failed("gateway.channel_budget 409", ErrorCode::Budget)?
        ),
        Ending::Failed {
            code: Some("budget".into()),
            refusal: Some("gateway.channel_budget 409".into())
        }
    );
    for reason in [
        "budget.output_tokens",
        "the tool printed the customer's file",
        "gateway.channel_budget",
        "gateway.x 409 y",
    ] {
        assert_eq!(
            Ending::of("run.failed", &failed(reason, ErrorCode::Budget)?),
            Ending::Failed {
                code: Some("budget".into()),
                refusal: None
            },
            "{reason}"
        );
    }
    Ok(())
}
