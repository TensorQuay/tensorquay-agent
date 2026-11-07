//! Ordering shared by durable append and the replay-to-live transition.

use crate::GatewayError;
use std::future::Future;
use tqa_types::{ErrorCode, Seq, SeqChecker};

pub(crate) async fn commit_then_publish(
    persist: impl Future<Output = Result<(), GatewayError>>,
    publish: impl FnOnce(),
) -> Result<(), GatewayError> {
    persist.await?;
    publish();
    Ok(())
}

pub(crate) struct ReplayCursor {
    last: Seq,
    watermark: Seq,
    checker: SeqChecker,
}

impl ReplayCursor {
    pub fn new(after: Seq, watermark: Seq) -> Result<Self, GatewayError> {
        if after > watermark {
            return Err(gap());
        }
        Ok(Self {
            last: after,
            watermark,
            checker: SeqChecker::new(after),
        })
    }

    pub fn last(&self) -> Seq {
        self.last
    }

    pub fn replay(&mut self, seq: Seq) -> Result<(), GatewayError> {
        if seq > self.watermark {
            return Err(gap());
        }
        self.accept(seq)
    }

    pub fn finish_replay(&self) -> Result<(), GatewayError> {
        if self.last != self.watermark {
            return Err(gap());
        }
        Ok(())
    }

    pub fn live(&mut self, seq: Seq) -> Result<bool, GatewayError> {
        self.finish_replay_or_live()?;
        // Only the replay snapshot can overlap the live queue. A later duplicate is a gap.
        if seq <= self.watermark {
            return Ok(false);
        }
        self.accept(seq)?;
        Ok(true)
    }

    fn finish_replay_or_live(&self) -> Result<(), GatewayError> {
        if self.last < self.watermark {
            return Err(gap());
        }
        Ok(())
    }

    fn accept(&mut self, seq: Seq) -> Result<(), GatewayError> {
        self.checker.accept(seq)?;
        self.last = seq;
        Ok(())
    }
}

pub(crate) const fn gap() -> GatewayError {
    GatewayError::Protocol(ErrorCode::SeqGap)
}

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod tests;
