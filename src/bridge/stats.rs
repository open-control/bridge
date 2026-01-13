//! Traffic statistics for the bridge
//!
//! Thread-safe counters for measuring bytes/sec throughput.
//! Uses lock-free atomics for all operations.

use crate::constants::RATE_UPDATE_MIN_INTERVAL_SECS;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Traffic statistics with rate calculation (fully lock-free)
pub struct Stats {
    /// Total bytes transmitted (to serial)
    tx_total: AtomicU64,
    /// Total bytes received (from serial)
    rx_total: AtomicU64,
    /// Snapshot of tx_total at last rate calculation
    tx_snapshot: AtomicU64,
    /// Snapshot of rx_total at last rate calculation
    rx_snapshot: AtomicU64,
    /// Reference instant for time calculations
    start_time: Instant,
    /// Nanoseconds since start_time at last rate calculation
    last_calc_nanos: AtomicU64,
    /// Cached TX rate in bytes/sec (stored as f64 bits)
    tx_rate: AtomicU64,
    /// Cached RX rate in bytes/sec (stored as f64 bits)
    rx_rate: AtomicU64,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            tx_total: AtomicU64::new(0),
            rx_total: AtomicU64::new(0),
            tx_snapshot: AtomicU64::new(0),
            rx_snapshot: AtomicU64::new(0),
            start_time: Instant::now(),
            last_calc_nanos: AtomicU64::new(0),
            tx_rate: AtomicU64::new(0),
            rx_rate: AtomicU64::new(0),
        }
    }

    /// Add transmitted bytes (Host -> Controller)
    #[inline]
    pub fn add_tx(&self, bytes: usize) {
        self.tx_total.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// Add received bytes (Controller -> Host)
    #[inline]
    pub fn add_rx(&self, bytes: usize) {
        self.rx_total.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// Get total transmitted bytes
    #[inline]
    #[allow(dead_code)] // Used in tests
    pub fn tx_bytes(&self) -> u64 {
        self.tx_total.load(Ordering::Relaxed)
    }

    /// Get total received bytes
    #[inline]
    #[allow(dead_code)] // Used in tests
    pub fn rx_bytes(&self) -> u64 {
        self.rx_total.load(Ordering::Relaxed)
    }

    /// Update rate calculations and return (tx_kb_s, rx_kb_s)
    /// Call this periodically (e.g., every 500ms) from the UI thread
    pub fn update_rates(&self) -> (f64, f64) {
        let now_nanos = self.start_time.elapsed().as_nanos() as u64;
        let last_nanos = self.last_calc_nanos.load(Ordering::Relaxed);
        let elapsed = (now_nanos - last_nanos) as f64 / 1_000_000_000.0;

        if elapsed < RATE_UPDATE_MIN_INTERVAL_SECS {
            // Too soon, return cached values
            let tx = f64::from_bits(self.tx_rate.load(Ordering::Relaxed));
            let rx = f64::from_bits(self.rx_rate.load(Ordering::Relaxed));
            return (tx, rx);
        }

        // Try to claim the update (avoid duplicate calculations)
        if self
            .last_calc_nanos
            .compare_exchange(last_nanos, now_nanos, Ordering::SeqCst, Ordering::Relaxed)
            .is_err()
        {
            // Another thread got there first, return cached values
            let tx = f64::from_bits(self.tx_rate.load(Ordering::Relaxed));
            let rx = f64::from_bits(self.rx_rate.load(Ordering::Relaxed));
            return (tx, rx);
        }

        let tx_now = self.tx_total.load(Ordering::Relaxed);
        let rx_now = self.rx_total.load(Ordering::Relaxed);
        let tx_prev = self.tx_snapshot.swap(tx_now, Ordering::Relaxed);
        let rx_prev = self.rx_snapshot.swap(rx_now, Ordering::Relaxed);

        let tx_rate = (tx_now - tx_prev) as f64 / elapsed / 1024.0; // KB/s
        let rx_rate = (rx_now - rx_prev) as f64 / elapsed / 1024.0; // KB/s

        self.tx_rate.store(tx_rate.to_bits(), Ordering::Relaxed);
        self.rx_rate.store(rx_rate.to_bits(), Ordering::Relaxed);

        (tx_rate, rx_rate)
    }
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}
