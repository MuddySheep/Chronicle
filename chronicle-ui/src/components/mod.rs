//! UI Components
//!
//! Reusable Leptos components for the dashboard.

pub mod nav;
pub mod chart;
pub mod metric_card;
pub mod data_entry;
pub mod insight_card;
pub mod loading;
pub mod toast;

pub use nav::Nav;
pub use chart::Chart;
pub use metric_card::MetricCard;
pub use data_entry::DataEntry;
pub use insight_card::InsightCard;
pub use loading::Loading;
pub use toast::Toast;
