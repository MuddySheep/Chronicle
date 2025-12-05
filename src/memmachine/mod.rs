//! MemMachine Integration
//!
//! Integrates Chronicle with MemMachine for pattern detection,
//! correlation analysis, and natural language insights.
//!
//! ## Architecture
//!
//! - **Client**: REST API client for MemMachine
//! - **SyncManager**: Periodic sync of daily summaries
//! - **InsightEngine**: Generate insights from questions
//! - **CorrelationEngine**: Calculate metric correlations
//!
//! ## Data Flow
//!
//! 1. Chronicle stores raw time-series data
//! 2. SyncManager sends daily summaries to MemMachine
//! 3. CorrelationEngine detects patterns and syncs them
//! 4. InsightEngine uses MemMachine context for questions

mod client;
mod correlations;
mod insights;
mod sync;

pub use client::{MemMachineClient, MemMachineConfig, MemMachineError, MemoryResult, SessionContext};
pub use correlations::{Correlation, CorrelationEngine};
pub use insights::{InsightEngine, InsightError, InsightResponse};
pub use sync::{SyncConfig, SyncManager, SyncState, SyncStatus};
