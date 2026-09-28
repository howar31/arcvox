//! Puts the refined text where the user wants it. `planner` decides,
//! `executors` act.

pub mod executors;
pub mod planner;

pub use executors::{run, Outcome};
pub use planner::{plan, Plan, ShowReason, Situation};
