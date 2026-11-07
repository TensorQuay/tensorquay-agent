//! The effective member cap (ADR-0034 point 3): members per lead at a time, never above the admission slots or the
//! per-user cap less one for the lead, so a lead and its members always fit (GW-I-196). Pure.

/// The cap and the limit that set it, as `member_limit` names it (`details: {limit, by}`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Cap {
    pub(crate) limit: u32,
    pub(crate) by: &'static str,
}

/// The least of `max_members`, `slots - 1` and `per_user - 1`. On a tie the admission limit is named, slots first: it
/// is the one an administrator would have to raise.
pub(crate) fn cap(max_members: u32, slots: u32, per_user: u32) -> Cap {
    [
        (slots.saturating_sub(1), "slots"),
        (per_user.saturating_sub(1), "per_user"),
        (max_members, "max_members"),
    ]
    .into_iter()
    .reduce(|least, next| if next.0 < least.0 { next } else { least })
    .map_or(
        Cap {
            limit: 0,
            by: "slots",
        },
        |(limit, by)| Cap { limit, by },
    )
}

#[cfg(test)]
mod tests {
    use super::{Cap, cap};

    #[test]
    fn gw_u_128_the_member_cap_is_the_least_limit_and_names_it() {
        let named = |limit, by| Cap { limit, by };
        // The defaults on a host of 8: max_members sets it.
        assert_eq!(cap(3, 8, 8), named(3, "max_members"));
        // GW-I-196's case: 4 slots and a per-user cap of 4 leave 3, named by the slots.
        assert_eq!(cap(3, 4, 4), named(3, "slots"));
        assert_eq!(cap(8, 4, 4), named(3, "slots"));
        assert_eq!(cap(8, 8, 3), named(2, "per_user"));
        assert_eq!(cap(8, 3, 3), named(2, "slots"));
        // One slot leaves no room for a member.
        assert_eq!(cap(3, 1, 1), named(0, "slots"));
        assert_eq!(cap(1, 8, 8), named(1, "max_members"));
    }
}
