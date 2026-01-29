//! D-Bus service for exposing CC Usage Widget data to external consumers like GNOME extensions.
//!
//! Exposes the `com.shane.CCUsageWidget1` interface at `/com/shane/CCUsageWidget`.

use crate::usage::get_current_usage;
use chrono::{Datelike, Utc};
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::{interface, Connection, Result};

/// D-Bus service providing usage summary data.
pub struct UsageService {
    /// Cached usage data to avoid recomputing on every D-Bus call
    cache: Arc<Mutex<Option<(f64, u32)>>>,
}

impl UsageService {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(None)),
        }
    }

    /// Update the cached usage data (called when file watcher detects changes)
    pub async fn update_cache(&self) {
        let data = Self::compute_usage_summary();
        let mut cache = self.cache.lock().await;
        *cache = Some(data);
    }

    /// Compute usage summary from current data
    fn compute_usage_summary() -> (f64, u32) {
        match get_current_usage("week") {
            Ok(stats) => {
                let week_usage_percent = stats.quota.week_usage_percent;
                let days_left = Self::compute_days_until_reset();
                (week_usage_percent, days_left)
            }
            Err(_) => (0.0, Self::compute_days_until_reset()),
        }
    }

    /// Compute days until the weekly reset (Sunday at midnight)
    fn compute_days_until_reset() -> u32 {
        let today = Utc::now().date_naive();
        let days_since_sunday = today.weekday().num_days_from_sunday();
        // Days until next Sunday (if today is Sunday, returns 7)
        let days_left = if days_since_sunday == 0 {
            7
        } else {
            7 - days_since_sunday
        };
        days_left
    }
}

#[interface(name = "com.shane.CCUsageWidget1")]
impl UsageService {
    /// Returns (week_usage_percent, days_left_until_reset)
    async fn get_usage_summary(&self) -> (f64, u32) {
        // Try to use cache first, fall back to computing
        let cache = self.cache.lock().await;
        if let Some(data) = *cache {
            return data;
        }
        drop(cache);

        // Cache miss - compute fresh data
        Self::compute_usage_summary()
    }
}

/// Handle to the running D-Bus service for updating cache
#[derive(Clone)]
pub struct DbusServiceHandle {
    service: Arc<UsageService>,
}

impl DbusServiceHandle {
    /// Notify the D-Bus service that usage data has changed
    pub async fn notify_usage_changed(&self) {
        self.service.update_cache().await;
    }
}

/// Initialize and run the D-Bus service on the session bus.
/// Returns a handle for updating the service cache.
pub async fn init_dbus_service() -> Result<DbusServiceHandle> {
    let service = Arc::new(UsageService::new());

    // Pre-populate the cache
    service.update_cache().await;

    let connection = Connection::session().await?;

    // Request the well-known bus name
    connection
        .request_name("com.shane.CCUsageWidget")
        .await?;

    // Register the object at the expected path
    connection
        .object_server()
        .at("/com/shane/CCUsageWidget", (*service).clone())
        .await?;

    // Keep the connection alive by spawning a task that holds it
    tokio::spawn(async move {
        // The connection stays alive as long as this task runs
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });

    Ok(DbusServiceHandle { service })
}

impl Clone for UsageService {
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
        }
    }
}
