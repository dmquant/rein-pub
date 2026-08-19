//! Injected time. rein-core has no clock at all; the runtime is where wall
//! time is allowed in, behind a trait, so every test can pin it.

use rein_core::time::{LogicalMs, Timestamp};

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
    /// Monotonic-ish milliseconds for event stamping and budget attribution.
    fn mono_ms(&self) -> LogicalMs;
}

/// Real time (production).
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before 1970")
            .as_millis() as i64;
        Timestamp::from_unix_millis(ms)
    }

    fn mono_ms(&self) -> LogicalMs {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        let start = *START.get_or_init(Instant::now);
        LogicalMs(start.elapsed().as_millis() as u64)
    }
}

/// Frozen time (tests, replay): deterministic by construction.
pub struct FixedClock {
    pub at: Timestamp,
    pub ms: std::sync::atomic::AtomicU64,
}

impl FixedClock {
    pub fn new(at: Timestamp) -> Self {
        Self {
            at,
            ms: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.at
    }

    fn mono_ms(&self) -> LogicalMs {
        // Strictly increasing so event order is well-defined, still fully
        // deterministic.
        LogicalMs(self.ms.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}
