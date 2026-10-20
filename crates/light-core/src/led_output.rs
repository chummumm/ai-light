//! LED delivery reconciliation, independent of task timers and sound episodes.
use crate::model::LightState;

const INITIAL_RETRY_MS: u64 = 5_000;
const MAX_RETRY_MS: u64 = 30_000;
const REFRESH_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedCommand {
    pub state: LightState,
    pub period_ms: u16,
    pub fade_ms: u16,
    pub serial: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedDelivery {
    Confirmed,
    Unconfirmed,
    Failed,
}

/// A successful system write does not prove that the lamp applied the command.
/// Retry uncertain writes with bounded backoff, and periodically reassert even
/// confirmed output because HID enumeration cannot reveal a silent device reset.
/// Callers must recompute the desired command before each attempt; no task event
/// or sound notification is replayed here. All times use a monotonic clock.
#[derive(Debug, Default)]
pub struct LedOutputSync {
    last_attempt: Option<LedCommand>,
    last_delivery: Option<LedDelivery>,
    next_attempt_ms: u64,
    retry_delay_ms: u64,
}

impl LedOutputSync {
    pub fn should_send(&self, desired: &LedCommand, now: u64) -> bool {
        // A new state or device must never inherit the old command's cooldown.
        self.last_attempt.as_ref() != Some(desired) || now >= self.next_attempt_ms
    }

    pub fn record_attempt(&mut self, desired: LedCommand, started_ms: u64, delivery: LedDelivery) {
        if self.last_attempt.as_ref() != Some(&desired) {
            self.retry_delay_ms = 0;
        }
        let delay = match delivery {
            LedDelivery::Confirmed => {
                self.retry_delay_ms = 0;
                REFRESH_MS
            }
            LedDelivery::Unconfirmed | LedDelivery::Failed => {
                self.retry_delay_ms = if self.retry_delay_ms == 0 {
                    INITIAL_RETRY_MS
                } else {
                    (self.retry_delay_ms * 2).min(MAX_RETRY_MS)
                };
                self.retry_delay_ms
            }
        };
        self.last_attempt = Some(desired);
        self.last_delivery = Some(delivery);
        // Anchor refreshes to the write attempt, not the time spent waiting for
        // its ACK, to avoid adding the ACK timeout to every breathing cycle.
        self.next_attempt_ms = started_ms.saturating_add(delay);
    }

    /// A failed probe invalidates confirmed output without postponing retries
    /// that are already pending. Successful probes never confirm LED delivery.
    pub fn transport_failed(&mut self, now: u64) {
        if self.last_delivery == Some(LedDelivery::Confirmed) {
            self.last_delivery = Some(LedDelivery::Failed);
            self.retry_delay_ms = INITIAL_RETRY_MS;
            self.next_attempt_ms = self
                .next_attempt_ms
                .min(now.saturating_add(INITIAL_RETRY_MS));
        }
    }

    /// Explicit refresh, device changes and resume must resend even the same Off.
    pub fn invalidate(&mut self) {
        *self = Self::default();
    }
}
