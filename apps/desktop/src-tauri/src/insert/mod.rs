//! Puts the refined text where the user wants it. `planner` decides,
//! `executors` (Task 8) act.

pub mod planner;

pub use planner::{plan, Plan, ShowReason, Situation};
