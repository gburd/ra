//! API endpoint handlers.

pub mod compare;
pub mod demos;
pub mod demos2;
pub mod execute;
pub mod explain;
pub mod hybrid;
pub mod isolation;
pub mod optimize;
pub mod rules;
pub mod share;
pub mod translate;
pub mod visualize;

#[cfg(test)]
mod cache_test;
#[cfg(test)]
mod execute_test;
#[cfg(test)]
mod explain_test;
