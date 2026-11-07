//! Journal an environment transition before handing its history to the core.
use super::Supervisor;
use crate::RuntimeError;
use tokio_util::sync::CancellationToken;
use tqa_core::{EventContext, EventSink};
use tqa_types::{Environment, Event, ServerFrame};

impl Supervisor {
    pub(super) async fn environment(
        &mut self,
        next: &Environment,
        shutdown: &CancellationToken,
    ) -> Result<(), RuntimeError> {
        let legacy = Environment::default();
        let (template, capabilities, version) = self
            .history
            .iter()
            .rev()
            .find_map(|frame| match frame {
                ServerFrame::Event {
                    event:
                        Event::EnvironmentChanged {
                            template,
                            capabilities,
                            version,
                        },
                    ..
                } => Some((template, capabilities, *version)),
                _ => None,
            })
            .unwrap_or((&legacy.template, &legacy.capabilities, None));
        // An image-only update of the same template is announced by its new version (revision 8, RT-I-029); a version
        // the gateway leaves out never announces one.
        let moved = next.version.is_some() && next.version != version;
        if template == &next.template && capabilities == &next.capabilities && !moved {
            return Ok(());
        }
        let seq = self
            .services
            .journal
            .emit(
                Event::EnvironmentChanged {
                    template: next.template.clone(),
                    capabilities: next.capabilities.clone(),
                    version: next.version,
                },
                EventContext::default(),
            )
            .await
            .map_err(|_| RuntimeError::io("environment event append"))?;
        // The sole writer has already enqueued this frame. Consume it before cloning history for a run.
        let frame = self
            .services
            .time
            .wait(
                self.services.limits.io_timeout,
                shutdown,
                self.frames.recv(),
            )
            .await?
            .ok_or(RuntimeError::Worker)?;
        if !matches!(&frame, ServerFrame::Event { seq: assigned, .. } if *assigned == seq) {
            return Err(RuntimeError::History);
        }
        self.record(frame)
    }
}
