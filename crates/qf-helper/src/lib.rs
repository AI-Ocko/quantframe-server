//! qf-helper: tells quantframe-server whether Warframe is running (spec §5.8, amendment D7)
//! and reports completed trades from EE.log (amendments E2–E3).

pub mod config;
pub mod ee_log;
pub mod heartbeat;
pub mod process;
pub mod queue;
pub mod trade;
