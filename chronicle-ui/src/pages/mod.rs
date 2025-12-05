//! Pages
//!
//! Top-level page components for each route.

pub mod dashboard;
pub mod metrics;
pub mod insights;
pub mod settings;

pub use dashboard::Dashboard;
pub use metrics::Metrics;
pub use insights::Insights;
pub use settings::Settings;
