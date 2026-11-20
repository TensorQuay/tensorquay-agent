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
            "New in channel {channel} since message 11: a proposal for your vote (message 12); a to-do you may \
             take on (message 13); 2 posts naming you (messages 14, 15); a reply to your post (message 16); the \
             decision on your proposal (message 17); a report on your to-do (message 18). Read them with \
             channel_read (channel {channel}, since 11); end your turn when you are waiting for more."
        )
    );
    assert_eq!(
        sentence(channel, 0, &[Waiting::Proposal(3), Waiting::Proposal(5)]),
        format!(
            "New in channel {channel} since message 0: 2 proposals for your vote (messages 3, 5). Read them with \
             channel_read (channel {channel}, since 0); end your turn when you are waiting for more."
        )
    );
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

// TEST: GW-U-231
#[test]
fn gw_u_231_the_wake_names_each_placed_files_exact_path() {
    assert_eq!(super::files_sentence(&[]), "");
    assert_eq!(
        super::files_sentence(&[(12, "uploads/pricing-12-q3.pdf".into())]),
        " The file of message 12 is in your workspace: uploads/pricing-12-q3.pdf."
    );
    assert_eq!(
        super::files_sentence(&[
            (12, "uploads/pricing-12-q3.pdf".into()),
            (12, "uploads/pricing-12-notes.txt".into()),
            (14, "uploads/pricing-14-chart.png".into()),
        ]),
        " Files of these messages are in your workspace: uploads/pricing-12-q3.pdf (message 12), \
         uploads/pricing-12-notes.txt (message 12), uploads/pricing-14-chart.png (message 14)."
    );
}
