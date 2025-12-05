//! Chronicle Query Engine
//!
//! Provides a SQL-like query language for time-series data:
//!
//! - **AST**: Query abstract syntax tree types
//! - **Parser**: Parse query strings into AST
//! - **Executor**: Execute queries against storage
//!
//! # Query Language
//!
//! ```text
//! SELECT metric [, metric2, ...]
//! [WHERE time >= now() - 7d]
//! [AND tags.location = 'office']
//! [GROUP BY day|hour|week|month]
//! [LIMIT n]
//! ```
//!
//! # Examples
//!
//! ## Using Query Builder
//!
//! ```rust,ignore
//! use chronicle::query::{Query, QueryExecutor, GroupByInterval, AggregationFunc};
//!
//! // Simple query
//! let query = Query::select(&["mood"]).last_days(7).build();
//!
//! // With aggregation
//! let query = Query::select(&["mood"])
//!     .last_days(30)
//!     .group_by(GroupByInterval::Day)
//!     .with_aggregation(AggregationFunc::Avg)
//!     .build();
//!
//! // Execute
//! let result = executor.execute(query).await?;
//! ```
//!
//! ## Using Query String
//!
//! ```rust,ignore
//! let result = executor.execute_str(
//!     "SELECT AVG(mood) WHERE time >= now() - 30d GROUP BY day"
//! ).await?;
//! ```

mod ast;
mod error;
mod executor;
mod parser;

pub use ast::{
    AggregationFunc, Filter, FilterField, FilterValue, GroupByClause, GroupByInterval, Operator,
    Query, QueryBuilder, SelectItem,
};
pub use error::{QueryError, QueryResult};
pub use executor::{QueryExecutor, QueryResult2 as QueryResultData, ResultRow};
pub use parser::parse_query;
