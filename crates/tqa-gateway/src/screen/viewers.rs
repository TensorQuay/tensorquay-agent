//! Who is watching a task's screen, who holds it, and which bridge each viewer gets (ADR-0039 sections 5 and 6). One
//! bridge per viewer: a viewer that opens asks for one, and a held bridge dial from the runtime takes the oldest ask.
//! At most four viewers and four bridges per task; the fifth of either is refused. Take over belongs to one viewer's
//! socket, and every viewer learns who holds the screen from one watch channel, which the control frames follow.
use std::{
    collections::{HashMap, VecDeque},
    sync::{LazyLock, Mutex},
    time::Instant,
};
use tokio::sync::{Notify, oneshot, watch};
use tqa_types::ConversationId;

/// At most this many viewers per task (the owner's tabs), and so this many bridges.
pub(super) const MAX_VIEWERS: usize = 4;

/// A viewer's socket, numbered within the process.
pub(super) type ViewerId = u64;

/// The other end a viewer is paired with: the bridge's upgraded socket, handed over once it exists.
pub(super) type Paired<T> = oneshot::Receiver<T>;

/// Who holds the screen, as every viewer sees it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Holder {
    #[default]
    Nobody,
    Viewer(ViewerId),
    /// Granted although the run's end reset wasn't confirmed within the wait: a sign-in now may not last.
    Late(ViewerId),
    /// Granted after a reset that couldn't empty the browser's profile: earlier sign-ins may still be there.
    Unclean(ViewerId),
}

/// The runtime's run-end resets as one task's screen has seen them (`screen.reset`, ADR-0039 section 6).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Resets {
    /// How many have come.
    pub(super) done: u64,
    /// A run has ended and its reset hasn't come yet.
    pub(super) pending: bool,
}

/// Why a viewer or a bridge dial is refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Full {
    /// Four viewers are open already (`screen_viewers`).
    Viewers,
    /// Four bridges are upgraded already (the runtime's dial is answered `409`).
    Bridges,
    /// Another viewer holds the screen: that tab is told so (`control` `other`), and keeps its socket.
    Taken,
}

/// One task's screen. `T` is a bridge's socket (generic so the pairing is tested without sockets).
pub(super) struct Screen<T> {
    viewers: Vec<ViewerId>,
    holder: Option<(ViewerId, Instant)>,
    /// Whether the holder has control yet: a Take over waits for the run's end reset (GW-U-155).
    granted: bool,
    resets: watch::Sender<Resets>,
    bridges: usize,
    /// The runtime's dials held now, waiting for a viewer to ask.
    holding: usize,
    asking: VecDeque<(ViewerId, oneshot::Sender<T>)>,
    asked: std::sync::Arc<Notify>,
    control: watch::Sender<Holder>,
}

impl<T> Default for Screen<T> {
    fn default() -> Self {
        Self {
            viewers: Vec::new(),
            holder: None,
            granted: false,
            resets: watch::channel(Resets::default()).0,
            bridges: 0,
            holding: 0,
            asking: VecDeque::new(),
            asked: std::sync::Arc::new(Notify::new()),
            control: watch::channel(Holder::Nobody).0,
        }
    }
}

impl<T> Screen<T> {
    /// A viewer opens: it's counted, asks for a bridge, and learns who holds the screen.
    pub(super) fn open(
        &mut self,
        viewer: ViewerId,
    ) -> Result<(Paired<T>, watch::Receiver<Holder>), Full> {
        if self.viewers.len() >= MAX_VIEWERS {
            return Err(Full::Viewers);
        }
        self.viewers.push(viewer);
        let (sender, receiver) = oneshot::channel();
        self.asking.push_back((viewer, sender));
        self.asked.notify_waiters();
        Ok((receiver, self.control.subscribe()))
    }

    /// A viewer's socket closed. When it held the screen, Take over ends: how long it lasted, for the notice.
    pub(super) fn close(&mut self, viewer: ViewerId, now: Instant) -> Option<std::time::Duration> {
        self.viewers.retain(|open| *open != viewer);
        self.asking.retain(|(asking, _)| *asking != viewer);
        self.hand_back(viewer, now)
    }

    /// The runtime's held dial takes the oldest viewer's ask, if any and if a bridge is free; `None` keeps holding.
    pub(super) fn take_ask(&mut self) -> Result<Option<oneshot::Sender<T>>, Full> {
        if self.bridges >= MAX_VIEWERS {
            return Err(Full::Bridges);
        }
        while let Some((_, sender)) = self.asking.pop_front() {
            if !sender.is_closed() {
                self.bridges += 1;
                return Ok(Some(sender));
            }
        }
        Ok(None)
    }

    /// A paired bridge ended.
    pub(super) fn bridge_closed(&mut self) {
        self.bridges = self.bridges.saturating_sub(1);
    }

    /// A dial is held: what it waits on, a viewer asking. The task stays known while a dial is held.
    pub(super) fn hold(&mut self) -> std::sync::Arc<Notify> {
        self.holding += 1;
        self.asked.clone()
    }

    /// A held dial ended (upgraded, answered `204`, or refused).
    pub(super) fn unhold(&mut self) {
        self.holding = self.holding.saturating_sub(1);
    }

    /// `viewer` takes over: refused while another holds it; again by the holder, nothing changes. It claims the screen,
    /// so no queued turn starts and no other tab takes over, but control (input, `you`) comes with [`Self::grant`].
    pub(super) fn take_over(&mut self, viewer: ViewerId, now: Instant) -> Result<bool, Full> {
        match self.holder {
            Some((holder, _)) if holder == viewer => Ok(false),
            Some(_) => Err(Full::Taken),
            None => {
                self.holder = Some((viewer, now));
                self.granted = false;
                Ok(true)
            }
        }
    }

    /// Gives `viewer` control, if it still holds the screen and hasn't had it yet; `ready` false says the run's end
    /// reset wasn't confirmed in time.
    #[cfg(test)]
    pub(super) fn grant(&mut self, viewer: ViewerId, ready: bool) -> bool {
        self.grant_as(viewer, ready, false)
    }

    /// [`Self::grant`], `unclean` when the task's last reset or sign-out couldn't empty the profile (GW-U-156).
    fn grant_as(&mut self, viewer: ViewerId, ready: bool, unclean: bool) -> bool {
        if self.granted || self.holder.is_none_or(|(holder, _)| holder != viewer) {
            return false;
        }
        self.granted = true;
        self.control.send_replace(if !ready {
            Holder::Late(viewer)
        } else if unclean {
            Holder::Unclean(viewer)
        } else {
            Holder::Viewer(viewer)
        });
        true
    }

    /// A run of the task has ended: its reset is to come.
    pub(super) fn run_ended(&mut self) {
        self.resets.send_modify(|resets| resets.pending = true);
    }

    /// The runtime's reset is over.
    pub(super) fn reset_done(&mut self) {
        self.resets.send_modify(|resets| {
            resets.done += 1;
            resets.pending = false;
        });
    }

    /// The run's end reset has landed: a late grant still held turns into plain control, or into control that says
    /// earlier sign-ins may remain when that reset couldn't empty the profile (`unclean`). Anything else is left as it
    /// is, so nobody is sent anything new (the CU review, #20).
    pub(super) fn settle_late(&mut self, unclean: bool) {
        let Holder::Late(viewer) = *self.control.borrow() else {
            return;
        };
        if self.granted && self.holder.is_some_and(|(holder, _)| holder == viewer) {
            self.control.send_replace(if unclean {
                Holder::Unclean(viewer)
            } else {
                Holder::Viewer(viewer)
            });
        }
    }

    /// Whether a run has ended and its reset hasn't come yet (the relay reads it from [`Self::resets`]).
    #[cfg(test)]
    pub(super) fn reset_pending(&self) -> bool {
        self.resets.borrow().pending
    }

    /// The resets as they come, for a Take over to wait on.
    pub(super) fn resets(&self) -> watch::Receiver<Resets> {
        self.resets.subscribe()
    }

    /// `viewer` hands back, if it holds the screen: how long it held it.
    pub(super) fn hand_back(
        &mut self,
        viewer: ViewerId,
        now: Instant,
    ) -> Option<std::time::Duration> {
        let (holder, since) = self.holder?;
        if holder != viewer {
            return None;
        }
        self.holder = None;
        self.granted = false;
        self.control.send_replace(Holder::Nobody);
        Some(now.duration_since(since))
    }

    /// Whether `viewer` holds the screen now (its input passes).
    pub(super) fn holds(&self, viewer: ViewerId) -> bool {
        self.granted && self.holder.is_some_and(|(holder, _)| holder == viewer)
    }

    /// Whether anyone holds the screen (no queued turn starts meanwhile, section 6).
    pub(super) fn taken_over(&self) -> bool {
        self.holder.is_some()
    }

    fn idle(&self) -> bool {
        self.viewers.is_empty() && self.bridges == 0 && self.holding == 0 && self.holder.is_none()
    }
}

/// Every task's screen in this process; a task's entry goes once nothing is open.
pub(super) struct Screens<T> {
    open: Mutex<HashMap<ConversationId, Screen<T>>>,
    /// Tasks whose last run-end reset or sign-out couldn't empty the browser's profile. Kept apart from the open
    /// screens, so a failure while nobody watches is still said at the next Take over; in memory only, so a gateway
    /// restart forgets it (GW-U-156).
    unclean: Mutex<std::collections::HashSet<ConversationId>>,
    /// Tasks whose run has ended and whose reset hasn't come yet. Kept apart from the open screens too, so a run that
    /// ends while nobody watches still holds a Take over pressed soon after until its reset (GW-U-196; before, the entry
    /// was dropped idle and the next one started with nothing pending: `sec_059_a`'s lost sign-in, c16f). A task's mark
    /// goes with its reset, its sandbox (no reset comes from one that's gone) or the task.
    due: Mutex<std::collections::HashSet<ConversationId>>,
    /// Tasks whose browser profile is known empty: a clean run-end reset or Sign out everywhere came, and no Take over
    /// since (a hand sign-in serves the run after it). Only these may give way to a waiting desktop (ADR-0039 section
    /// 10, amended 17 Oct 2026; GW-U-200). In memory only: after a restart none is known, which fails safe.
    known_empty: Mutex<std::collections::HashSet<ConversationId>>,
    /// Tasks whose last run-end reset said the browser didn't come back in time (GW-U-203, 16r's `sec_059_a`): a Take
    /// over is granted not ready, saying a sign-in now may not last, until a reset says the browser is ready. In memory
    /// only; a new sandbox starts its browser afresh.
    not_ready: Mutex<std::collections::HashSet<ConversationId>>,
}

impl<T> Default for Screens<T> {
    fn default() -> Self {
        Self {
            open: Mutex::new(HashMap::new()),
            unclean: Mutex::new(std::collections::HashSet::new()),
            due: Mutex::new(std::collections::HashSet::new()),
            known_empty: Mutex::new(std::collections::HashSet::new()),
            not_ready: Mutex::new(std::collections::HashSet::new()),
        }
    }
}

impl<T> Screens<T> {
    /// Runs `change` on `conversation`'s screen, dropping the entry once it's idle.
    pub(super) fn with<R>(
        &self,
        conversation: &ConversationId,
        change: impl FnOnce(&mut Screen<T>) -> R,
    ) -> Option<R> {
        let mut screens = self.open.lock().ok()?;
        let screen = screens.entry(conversation.clone()).or_insert_with(|| {
            let mut screen = Screen::default();
            if self.is_due(conversation) {
                screen.run_ended();
            }
            screen
        });
        let answer = change(screen);
        if screen.idle() {
            screens.remove(conversation);
        }
        Some(answer)
    }

    /// Changes `conversation`'s screen only if something of it is open: a task nobody watches gets no entry.
    pub(super) fn existing<R>(
        &self,
        conversation: &ConversationId,
        change: impl FnOnce(&mut Screen<T>) -> R,
    ) -> Option<R> {
        self.open.lock().ok()?.get_mut(conversation).map(change)
    }

    /// Whether `conversation`'s idle desktop may give way to a waiting one: its profile is known empty (GW-U-200).
    pub(super) fn may_retire(&self, conversation: &ConversationId) -> bool {
        !self.taken_over(conversation)
            && self
                .known_empty
                .lock()
                .is_ok_and(|tasks| tasks.contains(conversation))
    }

    fn set_known_empty(&self, conversation: &ConversationId, empty: bool) {
        if let Ok(mut tasks) = self.known_empty.lock() {
            if empty {
                tasks.insert(conversation.clone());
            } else {
                tasks.remove(conversation);
            }
        }
    }

    /// Whether anyone holds `conversation`'s screen.
    pub(super) fn taken_over(&self, conversation: &ConversationId) -> bool {
        self.open
            .lock()
            .is_ok_and(|screens| screens.get(conversation).is_some_and(Screen::taken_over))
    }

    /// A run of `conversation` has ended: its reset is due, whether or not anything of its screen is open.
    pub(super) fn run_ended(&self, conversation: &ConversationId) {
        self.set_due(conversation, true);
        self.existing(conversation, Screen::run_ended);
    }

    /// The runtime's run-end reset is over: a Take over waiting for it is woken, a late grant is settled, and `ok` false
    /// marks the task unclean until a clean reset or sign-out. With `ready` false the browser didn't come back in time:
    /// a late grant stays late, and the task is marked so a later Take over is granted not ready too (GW-U-203).
    pub(super) fn reset_done(&self, conversation: &ConversationId, ok: bool, ready: bool) {
        self.mark(conversation, !ok);
        self.set_known_empty(conversation, ok);
        self.set_due(conversation, false);
        Self::set(&self.not_ready, conversation, !ready);
        self.existing(conversation, |screen| {
            screen.reset_done();
            if ready {
                screen.settle_late(!ok);
            }
        });
    }

    /// Puts `conversation` in or out of one of the task marks.
    fn set(
        marks: &Mutex<std::collections::HashSet<ConversationId>>,
        conversation: &ConversationId,
        on: bool,
    ) {
        if let Ok(mut tasks) = marks.lock() {
            if on {
                tasks.insert(conversation.clone());
            } else {
                tasks.remove(conversation);
            }
        }
    }

    /// Sign out everywhere was asked of the runtime: its browser closes and starts again, as at a run's end, so a Take
    /// over meanwhile waits for the answer as it would for that run's reset (GW-U-204).
    pub(super) fn sign_out_asked(&self, conversation: &ConversationId) {
        self.run_ended(conversation);
    }

    /// Sign out everywhere's answer, given once the browser has started again: as a run's end reset, an emptied profile
    /// leaves nothing to warn of and a failure leaves the task unclean, and with `ready` false a Take over is granted not
    /// ready until a reset or sign-out says the browser is back (GW-U-204; the 16v integration's `sec_059_a` review).
    pub(super) fn signed_out(&self, conversation: &ConversationId, ok: bool, ready: bool) {
        self.reset_done(conversation, ok, ready);
    }

    /// Forgets that `conversation`'s profile may hold sign-ins: a new sandbox starts with an empty profile (it lives
    /// inside the sandbox), and a deleted task has none (the takeover pair's review, P3).
    pub(super) fn forget(&self, conversation: &ConversationId) {
        self.mark(conversation, false);
        self.set_due(conversation, false);
        Self::set(&self.not_ready, conversation, false);
        // A new sandbox starts empty, but nothing has said so yet: known again at its first reset.
        self.set_known_empty(conversation, false);
    }

    /// `conversation`'s sandbox is gone: no reset will come from it. This also bounds the due marks, since a task whose
    /// environment has no desktop never sends one.
    pub(super) fn sandbox_gone(&self, conversation: &ConversationId) {
        self.set_due(conversation, false);
        self.set_known_empty(conversation, false);
        // Its browser goes with it; the next sandbox's first reset says how the new one is.
        Self::set(&self.not_ready, conversation, false);
    }

    fn is_due(&self, conversation: &ConversationId) -> bool {
        self.due
            .lock()
            .is_ok_and(|tasks| tasks.contains(conversation))
    }

    fn set_due(&self, conversation: &ConversationId, due: bool) {
        if let Ok(mut tasks) = self.due.lock() {
            if due {
                tasks.insert(conversation.clone());
            } else {
                tasks.remove(conversation);
            }
        }
    }

    /// Whether `conversation`'s profile is marked as possibly holding sign-ins (GW-U-156's gateway cases).
    #[cfg(feature = "testing")]
    pub(super) fn is_unclean(&self, conversation: &ConversationId) -> bool {
        self.unclean
            .lock()
            .is_ok_and(|tasks| tasks.contains(conversation))
    }

    fn mark(&self, conversation: &ConversationId, unclean: bool) {
        if let Ok(mut tasks) = self.unclean.lock() {
            if unclean {
                tasks.insert(conversation.clone());
            } else {
                tasks.remove(conversation);
            }
        }
    }

    /// Gives `viewer` control of `conversation`'s screen, saying so when the task is unclean.
    pub(super) fn grant(
        &self,
        conversation: &ConversationId,
        viewer: ViewerId,
        ready: bool,
    ) -> Option<bool> {
        let unclean = self
            .unclean
            .lock()
            .is_ok_and(|tasks| tasks.contains(conversation));
        // A browser the last reset said wasn't back yet: control, but a sign-in now may not last (GW-U-203).
        let ready = ready
            && !self
                .not_ready
                .lock()
                .is_ok_and(|tasks| tasks.contains(conversation));
        // A person has the screen: a sign-in may follow, and it serves the next run (GW-U-200).
        self.set_known_empty(conversation, false);
        self.with(conversation, |screen| {
            screen.grant_as(viewer, ready, unclean)
        })
    }
}

/// Whether the run-end reset a Take over waits for is over (GW-U-155): at once when no run was cancelled and none is
/// due; else once none is pending and, for a cancelled run, one has come since `before`. False if the screen is gone.
pub(super) async fn reset_over(
    mut resets: watch::Receiver<Resets>,
    before: u64,
    cancelled: bool,
) -> bool {
    if !cancelled && !resets.borrow().pending {
        return true;
    }
    resets
        .wait_for(|resets| !resets.pending && (!cancelled || resets.done > before))
        .await
        .is_ok()
}

/// A process-wide counter for viewer ids.
pub(super) fn next_viewer() -> ViewerId {
    static NEXT: LazyLock<std::sync::atomic::AtomicU64> =
        LazyLock::new(|| std::sync::atomic::AtomicU64::new(1));
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
#[path = "viewers_tests.rs"]
mod tests;
