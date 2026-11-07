//! Registration and action decisions run only in the supervisor task.
use super::Supervisor;
use crate::{
    RuntimeError, RuntimeExit,
    link::{self, LinkEvent},
    session::Session,
};
use tokio_util::sync::CancellationToken;
use tqa_core::UserTurn;
use tqa_types::{ClientFrame, ErrorCode, Seq, ServerFrame};

impl Supervisor {
    pub(super) async fn action(
        &mut self,
        action: LinkEvent,
        shutdown: &CancellationToken,
    ) -> Result<Option<RuntimeExit>, RuntimeError> {
        match action {
            LinkEvent::Register {
                session,
                watermark,
                ready,
            } => {
                let result = self.register(session, watermark, shutdown).await;
                let failure = result.as_ref().err().cloned();
                // The peer can disconnect while the history request is in progress.
                if ready.send(result).is_err() {
                    tracing::debug!("registration peer disconnected before replay");
                }
                if let Some(error) = failure {
                    return Err(error);
                }
            }
            LinkEvent::Action(ClientFrame::UserMessage {
                lead_brief,
                team_wake,
                text,
                attachments,
                host_keys,
            }) => {
                let turn = UserTurn {
                    text,
                    attachments,
                    lead_brief: lead_brief.map(Box::new),
                    team_wake,
                };
                // The run's host keys go only to that run's processes and prompt (ADR-0037 §3).
                if self.bootstrap_idle && self.pending_message.is_none() {
                    self.pending_message = Some((turn, host_keys));
                } else if !self.bootstrap_idle && self.runs.is_empty() {
                    self.start(Some(turn), host_keys)?;
                } else {
                    self.error(ErrorCode::Busy, "a run is already active", true);
                }
            }
            LinkEvent::Action(ClientFrame::SessionUpdate {
                route,
                fallback_routes,
            }) => {
                let session = self.session.as_mut().ok_or(RuntimeError::Worker)?;
                session.route = route;
                session.fallback_routes = fallback_routes;
            }
            LinkEvent::Action(ClientFrame::RunCancel { run_id }) => {
                if self.active.as_ref() == Some(&run_id) {
                    self.cancel.cancel();
                } else {
                    tracing::debug!("ignored cancellation for an inactive run");
                }
            }
            LinkEvent::Action(ClientFrame::WorkspaceRequest { request_id, op }) => {
                self.workspace.request(request_id, op);
            }
            LinkEvent::Action(ClientFrame::WorkspaceCancel { request_id }) => {
                self.workspace.cancel(request_id);
            }
            // The store's answer to a screenshot upload; one nobody waits for any more is ignored (ADR-0039 §9).
            LinkEvent::Action(ClientFrame::ScreenStored { id, ok, reason }) => {
                self.services.screen_store.answered(&id, ok, reason);
            }
            LinkEvent::Action(ClientFrame::ScreenSignOut {}) => self.screen_sign_out().await,
            LinkEvent::Action(_) => self.error(
                ErrorCode::InvalidFrame,
                "this action is not supported on the runtime link",
                false,
            ),
        }
        Ok(None)
    }
    /// The desktop browser's profile: `/run/tqa-browser/profile`, or a test's own directory.
    #[cfg_attr(
        not(feature = "testing"),
        expect(
            clippy::unused_self,
            reason = "only a test's own profile directory is read from the supervisor"
        )
    )]
    fn browser_profile(&self) -> std::path::PathBuf {
        #[cfg(feature = "testing")]
        if let Some(path) = self
            .testing
            .browser_profile
            .lock()
            .ok()
            .and_then(|path| path.clone())
        {
            return path;
        }
        tqa_tools::builtin::computer::cdp::profile_dir()
    }

    /// Every run of a desktop session starts signed out (ADR-0039 section 6, the lead's decision of 7 Oct 2026): when a
    /// run ends, the browser closes and its profile is emptied. A sign-in made during a Take over, after the run it
    /// cancelled has ended, so serves the run that follows it, and no later one. Once the reset is over the gateway is
    /// told (`screen.reset`), and only then gives a Take over control (RT-I-051).
    pub(super) async fn signed_out_after_run(&mut self) {
        if !self.desktop() {
            return;
        }
        let (ok, ready) = match crate::desktop::sign_out(
            &self.services.desktop,
            &self.browser_profile(),
            crate::desktop::RELEASE_WAIT,
            &self.services.cleanup.supervised(),
        )
        .await
        {
            Ok(()) => (true, self.reopen_browser().await),
            Err(reason) => {
                tracing::warn!(
                    ?reason,
                    "desktop: the browser's profile wasn't emptied after the run"
                );
                (false, false)
            }
        };
        self.control(ServerFrame::ScreenReset { ok, ready });
    }

    /// Whether this session's environment declares the desktop.
    fn desktop(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| crate::screen_bridge::wanted(&session.environment))
    }

    /// The browser started again, blank, on the emptied profile (ADR-0039 section 6), so a Take over finds it.
    async fn reopen_browser(&self) -> bool {
        #[cfg(feature = "testing")]
        if let Some(profile) = self
            .testing
            .browser_profile
            .lock()
            .ok()
            .and_then(|path| path.clone())
        {
            if let Err(error) = std::fs::create_dir_all(&profile) {
                tracing::warn!(%error, "testing: the profile directory wasn't made");
            }
            let wait = self
                .testing
                .browser_reopen_ms
                .load(std::sync::atomic::Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            self.testing
                .browser_reopened
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return !self
                .testing
                .browser_not_ready
                .load(std::sync::atomic::Ordering::SeqCst);
        }
        crate::desktop::reopen(&self.services.desktop, &self.services.cleanup.supervised()).await
    }

    /// Sign out everywhere (ADR-0039 section 6): sent only with no run active; one still going is in use.
    async fn screen_sign_out(&mut self) {
        let result = if self.runs.is_empty() {
            crate::desktop::sign_out(
                &self.services.desktop,
                &self.browser_profile(),
                crate::desktop::RELEASE_WAIT,
                &self.services.cleanup.supervised(),
            )
            .await
        } else {
            Err(tqa_types::SignOutFailure::ProfileInUse)
        };
        // Answered once the browser has started again, with whether it is ready for a person's keys, as a run's end
        // reset is (RT-I-056): answered first, a Take over right after was granted plain over a desktop with no browser
        // yet. A failed sign-out left the browser as it was, and reopening finds it up.
        let ready = !self.desktop() || self.reopen_browser().await;
        self.control(ServerFrame::ScreenSignOutDone {
            ok: result.is_ok(),
            reason: result.err(),
            ready,
        });
    }
    async fn register(
        &mut self,
        session: Session,
        watermark: Seq,
        shutdown: &CancellationToken,
    ) -> Result<(), RuntimeError> {
        if let Some(current) = &mut self.session {
            current.compare_frozen(&session)?;
            self.services.journal.register(watermark).await?;
            current.route = session.route;
            current.mcp = session.mcp;
            current.network = session.network;
            return Ok(());
        }
        if session.protocol != 1 {
            return Err(RuntimeError::Frame);
        }
        let first_page = self.services.journal.register(watermark).await?;
        let tail = self.load_tail(first_page).await?;
        let stored = link::history::load(
            &self.services.config,
            watermark,
            self.services.limits,
            &self.services.time,
            shutdown,
        )
        .await?;
        let merged = crate::merge_history(stored, tail, watermark)?;
        for frame in merged {
            self.record(frame)?;
        }
        self.environment(&session.environment, shutdown).await?;
        // A desktop session's screen bridge dials beside the link from its first registration; the environment is
        // frozen for the session, so it starts once (ADR-0039 section 5, RT-I-045).
        if crate::screen_bridge::wanted(&session.environment) && self.screen.is_empty() {
            self.screen.spawn(
                crate::screen_bridge::ScreenBridge {
                    config: self.services.config.clone(),
                    pace: crate::screen_bridge::Pace::default(),
                    vnc: crate::screen_bridge::VNC,
                    jitter: Box::new(tqa_llm::ThreadRngJitter),
                    shutdown: self.link_shutdown.child_token(),
                }
                .run(),
            );
        }
        self.bootstrap_idle = matches!(
            tqa_context::resume_point(&self.history).map_err(|_| RuntimeError::History)?,
            tqa_context::ResumePoint::Idle { .. }
        );
        // A run interrupted by the runtime's restart resumes with its keys, newly minted for this sandbox (G4b).
        let host_keys = session.run_host_keys.clone();
        self.session = Some(session);
        self.start(None, host_keys)?;
        tracing::info!("runtime session registered");
        Ok(())
    }
    async fn load_tail(
        &self,
        mut page: Vec<(Seq, ServerFrame)>,
    ) -> Result<Vec<(Seq, ServerFrame)>, RuntimeError> {
        let mut records = Vec::new();
        let mut bytes = 0_usize;
        while let Some((cursor, _)) = page.last() {
            let cursor = *cursor;
            for record in page {
                bytes = self.check_tail_bound(&record.1, records.len(), bytes)?;
                records.push(record);
            }
            page = self.services.journal.records_after(cursor).await?;
        }
        Ok(records)
    }
    fn check_tail_bound(
        &self,
        frame: &ServerFrame,
        records: usize,
        bytes: usize,
    ) -> Result<usize, RuntimeError> {
        let bytes = bytes.saturating_add(
            serde_json::to_vec(frame)
                .map_err(|_| RuntimeError::Frame)?
                .len(),
        );
        if records >= self.services.limits.history_frames
            || bytes > self.services.limits.history_bytes
        {
            return Err(RuntimeError::Limit);
        }
        Ok(bytes)
    }
    pub(super) fn error(&self, code: ErrorCode, message: &str, retryable: bool) {
        self.control(ServerFrame::Error {
            target: None,
            seq: None,
            code,
            message: message.into(),
            retryable,
        });
    }
}
