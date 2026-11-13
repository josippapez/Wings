//! Shows the Azure Boards work item named in each pane's branch, like `feature/1234-login`, with its state, sprint and
//! parent, and moves it to its next state. Opens a work item in a tab with its description, comments and linked pull
//! requests, and lists the work items assigned to you in the current sprint under each project.
//!
//! The crate builds three frames. The main script (`--features main`, `main_frame.rs`) sets the badges and the Work
//! items sections. The item tab (`--features item`, `item_page.rs`) and the Sprint block (`--features sprint`,
//! `sprint_page.rs`) run `az` themselves.

// Each frame uses its own part of the crate.
#![allow(dead_code)]

mod az;
mod branch;
mod html;
mod item;
#[cfg(feature = "item")]
mod item_page;
#[cfg(feature = "main")]
mod main_frame;
mod parts;
#[cfg(feature = "main")]
mod project_sections;
mod sections;
#[cfg(feature = "sprint")]
mod sprint_page;
mod type_color;
mod urls;
mod view;
