//! Host keys (ADR-0037): a secret for one web address, used by a task through the egress proxy without ever entering
//! its sandbox. This module holds the store and its API; the proxy's side comes in later slices.
mod admin;
mod api;
mod from_git;
mod notices;
mod placeholders;
mod rules;
mod run_keys;
mod task_uses;
mod turn;
mod usage;
mod use_rule;
mod uses;

pub(crate) use admin::{
    admin_add, admin_list, admin_put, admin_reads, admin_remove, reads, shared_list,
};
pub(crate) use api::{add, list, put, remove};
pub(crate) use from_git::{copy_git_credentials, kept as git_credentials_kept};
pub(crate) use notices::{Call, Notices, for_viewer, notify};
pub(crate) use placeholders::{Bound, find};
pub(crate) use placeholders::{
    bind_run, drop_delivery, end_run, end_sandbox, end_tasks, sweep as sweep_ended,
};
pub(crate) use task_uses::task_uses;
pub(crate) use turn::{for_resume, for_turn, would_have_keys};
pub(crate) use usage::{Use, refuse, value};
pub(crate) use use_rule::{Caller, Form, Outcome, Placement, judge, marked, placement};

/// What the gateway keeps in memory about keys in use: which notices went out, and the values its swaps taught the
/// proxy's scrubbers.
#[derive(Default)]
pub(crate) struct Live {
    pub(crate) notices: Notices,
    pub(crate) scrubbers: crate::egress::Scrubbers,
}

impl Live {
    /// Forgets what the proxy's scrubbers learned for a changed or deleted key's old address; none for an answer
    /// replayed from an earlier request, which forgot it then.
    pub(crate) fn forget(&self, address: Option<&(String, i32)>) {
        if let Some((host, port)) = address
            && let Ok(port) = u16::try_from(*port)
        {
            self.scrubbers.forget(host, port);
        }
    }
}
