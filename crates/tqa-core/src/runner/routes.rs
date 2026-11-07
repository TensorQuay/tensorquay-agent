//! The conversation's route and its admin-declared fallback routes (ADR-0025).
//! A run switches to the next route when the active one stays unavailable, and stays there.
use super::session::{Execution, Stop};
use crate::{CoreError, limits::MODEL_ATTEMPTS};
use std::{sync::Arc, time::Duration};
use tqa_llm::{LlmError, ModelProvider, ThrottleScope};
use tqa_types::{Event, RouteSnapshot, ServerFrame};

/// A server-requested delay longer than this switches at once instead of waiting.
const SWITCH_AFTER: Duration = Duration::from_mins(1);
/// The reason recorded when a recorded fallback is no longer configured.
const UNAVAILABLE: &str = "route_unavailable";

/// A route snapshot with the provider that reaches it through the relay.
pub struct RouteProvider {
    /// The secret-free route snapshot.
    pub route: RouteSnapshot,
    /// The adapter for that route.
    pub provider: Arc<dyn ModelProvider>,
}

/// Checks each fallback: a valid snapshot, a matching provider and a unique name that differs from
/// the conversation's route.
pub(super) fn validate(own: &RouteSnapshot, fallbacks: &[RouteProvider]) -> Result<(), CoreError> {
    if fallbacks.len() > tqa_types::MAX_FALLBACK_ROUTES {
        return Err(CoreError::InvalidConfiguration("too many fallback routes"));
    }
    for (index, fallback) in fallbacks.iter().enumerate() {
        fallback
            .route
            .validate()
            .map_err(|_| CoreError::InvalidConfiguration("fallback route snapshot"))?;
        let provider = fallback.provider.route();
        if provider.name != fallback.route.name || provider.model != fallback.route.model {
            return Err(CoreError::InvalidConfiguration(
                "fallback provider does not match its route",
            ));
        }
        if fallback.route.name == own.name
            || fallbacks[..index]
                .iter()
                .any(|earlier| earlier.route.name == fallback.route.name)
        {
            return Err(CoreError::InvalidConfiguration("duplicate fallback route"));
        }
    }
    Ok(())
}

/// Whether an error should move the run to the next route: an availability failure whose retry
/// budget is spent (`exhausted`), or one that asks to wait longer than [`SWITCH_AFTER`].
pub(super) fn should_switch(error: &LlmError, exhausted: bool) -> bool {
    let long_wait = matches!(
        error,
        LlmError::RateLimited { retry_after: Some(delay) }
            | LlmError::Unavailable { retry_after: Some(delay) }
            | LlmError::GatewayRefused { retry_after: Some(delay), .. } if *delay > SWITCH_AFTER
    );
    error.retryable() && (exhausted || long_wait)
}

/// Whether the relay's own throttle moves the run to the next route. The user's rate is shared by every route, so a
/// user throttle never switches; a route's queue switches when it asks to wait longer than [`SWITCH_AFTER`], or once
/// it has refused a whole retry budget in a row.
pub(super) fn should_switch_throttled(
    scope: ThrottleScope,
    retry_after: Duration,
    in_a_row: u32,
) -> bool {
    scope == ThrottleScope::Route && (retry_after > SWITCH_AFTER || in_a_row >= MODEL_ATTEMPTS)
}

impl Execution<'_> {
    /// The route the run currently uses.
    pub(super) fn route(&self) -> &RouteSnapshot {
        self.active_fallback()
            .map_or(&self.runner.route, |fallback| &fallback.route)
    }

    /// The conversation's own route.
    pub(super) fn own_route(&self) -> &RouteSnapshot {
        &self.runner.route
    }

    /// The provider of the active route.
    pub(super) fn provider(&self) -> &Arc<dyn ModelProvider> {
        self.active_fallback()
            .map_or(&self.runner.provider, |fallback| &fallback.provider)
    }

    /// The active fallback; `None` while the run uses its own route (index 0).
    fn active_fallback(&self) -> Option<&RouteProvider> {
        self.active_route
            .checked_sub(1)
            .and_then(|index| self.runner.fallbacks.get(index))
    }

    /// Restores the active route from this run's journal once per invocation. A recorded fallback
    /// that is no longer configured returns the run to its own route, with a `route.fallback` notice.
    pub(super) async fn sync_route(&mut self) -> Result<(), Stop> {
        if self.route_synced {
            return Ok(());
        }
        self.route_synced = true;
        // The run's latest route: a failover's target, or the route a decision engine chose (ADR-0013 point 2).
        let recorded = self.frames.iter().rev().find_map(|frame| match frame {
            ServerFrame::Event {
                run_id,
                event: Event::RouteFallback { to, .. },
                ..
            } if *run_id == self.context.run_id => Some(to.clone()),
            ServerFrame::Event {
                run_id,
                event:
                    Event::Decision {
                        choice: tqa_types::DecisionChoice::CallModel { route, .. },
                        ..
                    },
                ..
            } if *run_id == self.context.run_id => Some(route.clone()),
            _ => None,
        });
        let Some(name) = recorded else {
            return Ok(());
        };
        if name == self.runner.route.name {
            return Ok(());
        }
        if let Some(index) = self
            .runner
            .fallbacks
            .iter()
            .position(|fallback| fallback.route.name == name)
        {
            self.active_route = index + 1;
            return Ok(());
        }
        let own = self.runner.route.name.clone();
        self.emit(Event::RouteFallback {
            from: name,
            to: own,
            reason: UNAVAILABLE.into(),
        })
        .await?;
        Ok(())
    }

    /// Moves to the next route, recording why; false when no route is left.
    pub(super) async fn switch_route(&mut self, reason: String) -> Result<bool, Stop> {
        let Some(next) = self.runner.fallbacks.get(self.active_route) else {
            return Ok(false);
        };
        let (from, to) = (self.route().name.clone(), next.route.name.clone());
        self.emit(Event::RouteFallback { from, to, reason }).await?;
        self.active_route += 1;
        Ok(true)
    }
}
