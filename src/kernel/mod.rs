pub mod api;

pub mod vfs;

pub mod protocol;

pub mod handlers;

#[cfg(feature = "metrics")]
pub mod metrics;

#[cfg(not(feature = "metrics"))]
pub mod metrics {
    pub fn init() {} // No-op when metrics are disabled
}