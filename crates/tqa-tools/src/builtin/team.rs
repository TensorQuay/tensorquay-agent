//! Agent team tools (ADR-0034 slice 3, the S1 contract's tool table): a member's `send_to_lead` and `wait_for_lead`,
//! and the lead's `start_member`, `assign`, `ask_all`, `decide`, `ask_person` and `read_reports` (`take_files`,
//! `stop_member`, `read_helper` and `done` come with the slices that build their routes). The run's role decides which
//! it gets ([`tools_for_role`]); a task that may lead gets the lead's tools but is offered only `start_member` until
//! its first one succeeds (TOOL-U-037).
mod client;
mod done;
mod tools;
pub use client::{ReadAnswer, TeamClient, TeamFailure};
pub use done::{ACCEPTANCE_TIMEOUT, AcceptanceShell};
pub use tools::{TeamRole, grouped, take_files, tools_for_role, tools_with_check};
