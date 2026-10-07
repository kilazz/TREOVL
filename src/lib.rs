#![allow(non_snake_case)]

pub mod cli;
pub mod engine;
#[cfg(feature = "gui")]
pub mod gui;
pub mod utils;

#[cfg(feature = "gui")]
slint::include_modules!();
