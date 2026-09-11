//! Native resource sampling. Each sensor owns its OS handles and refresh policy;
//! product aggregation and desktop scheduling stay in their respective layers.
pub mod memory;
pub mod release;
