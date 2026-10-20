//! Apply LED changes without restarting an effect that may already be running.
use crate::model::LightState;
use serde::{Deserialize, Serialize};

const INITIAL_RETRY_MS: u64 = 5_000;
const MAX_RETRY_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedCommand {
    pub state: LightState,
    pub period_ms: u16,
    pub fade_ms: u16,
    pub serial: String,
}

impl LedCommand {
    fn same_effect(&self, other: &Self) -> bool {
        self.state == other.state
            && self.serial == other.serial
            && (!matches!(self.state, LightState::Working | LightState::Error)
                || (self.period_ms == other.period_ms && self.fade_ms == other.fade_ms))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedDelivery {
    Confirmed,
    /// The effect may be running. Missing ACKs or a lost worker reply do not
    /// justify repeating a command that restarts the breathing phase.
    Unconfirmed,
    /// The worker did not begin submitting the LED command, or the device
    /// explicitly rejected it. Only this outcome retries.
    Failed,
}

/// A successful system write does not prove that the lamp applied the command.
/// Repeating an applied breathing command is observable, so confirmed and
/// uncertain attempts stay quiet until the desired effect or connection changes.
/// Retry only explicit failures, always recomputing the current target rather
/// than replaying an old task event. All times use a monotonic clock.
#[derive(Debug, Default)]
pub struct LedOutputSync {
    last_attempt: Option<LedCommand>,
    retry_at_ms: Option<u64>,
    retry_delay_ms: u64,
    disconnected: bool,
}

impl LedOutputSync {
    pub fn should_send(&self, desired: &LedCommand, now: u64) -> bool {
        !self.disconnected
            && (self
                .last_attempt
                .as_ref()
                .is_none_or(|last| !last.same_effect(desired))
                || self.retry_at_ms.is_some_and(|deadline| now >= deadline))
    }

    pub fn record_attempt(&mut self, desired: LedCommand, started_ms: u64, delivery: LedDelivery) {
        if self
            .last_attempt
            .as_ref()
            .is_none_or(|last| !last.same_effect(&desired))
        {
            self.retry_delay_ms = 0;
        }
        self.retry_at_ms = match delivery {
            LedDelivery::Confirmed | LedDelivery::Unconfirmed => {
                self.retry_delay_ms = 0;
                None
            }
            LedDelivery::Failed => {
                self.retry_delay_ms = if self.retry_delay_ms == 0 {
                    INITIAL_RETRY_MS
                } else {
                    (self.retry_delay_ms * 2).min(MAX_RETRY_MS)
                };
                Some(started_ms.saturating_add(self.retry_delay_ms))
            }
        };
        self.last_attempt = Some(desired);
    }

    /// Observe an actual OS connection state, not a battery/READ timeout.
    /// Unknown observations preserve the last evidence. Only a disconnected
    /// -> connected edge requests one fresh application of the latest target.
    pub fn observe_connection(&mut self, connected: Option<bool>) -> bool {
        match connected {
            Some(false) => self.disconnected = true,
            Some(true) if self.disconnected => {
                self.disconnected = false;
                self.invalidate();
                return true;
            }
            _ => {}
        }
        false
    }

    /// Explicit refresh or actual resume requests even an unchanged Off once.
    /// Do not discard evidence that the selected device is still disconnected.
    pub fn invalidate(&mut self) {
        self.last_attempt = None;
        self.retry_at_ms = None;
        self.retry_delay_ms = 0;
    }
}
