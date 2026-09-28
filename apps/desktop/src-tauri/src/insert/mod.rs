//! Puts the refined text where the user wants it. `planner` decides,
//! `executors` act.

pub mod planner;

pub use planner::{plan, Plan, ShowReason, Situation};
