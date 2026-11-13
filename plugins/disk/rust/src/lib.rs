//! Shows what takes the space on your disks as a map you click into, like DaisyDisk: scan a disk or a folder, find the
//! biggest folders and files, gather them in the Collector and move them to the Trash. Claude can scan too, with its
//! disk tools.
//!
//! The crate builds two frames. The main script (`--features main`, `main_frame.rs`) runs every scan, keeps the Collector,
//! does what the tab asks on the disk and answers Claude's tools. The Disk usage tab (`--features map`, `map_page.rs`) asks
//! it for the scans and the Collector, and Wings for the folder it shows: Wings keeps each scan's tree, so neither frame
//! holds one (`level.rs`).

// Each frame uses its own part of the crate.
#![allow(dead_code)]

#[cfg(feature = "main")]
mod actions;
#[cfg(feature = "main")]
mod answers;
#[cfg(feature = "map")]
mod ask;
mod bytes;
#[cfg(feature = "main")]
mod collector;
mod df;
mod disks;
mod entry;
mod gate;
mod level;
#[cfg(feature = "main")]
mod main_frame;
#[cfg(feature = "map")]
mod map_page;
#[cfg(feature = "map")]
mod map_view;
mod paths;
mod protect;
mod rows;
#[cfg(feature = "main")]
mod scans;
mod state;
mod sunburst;
#[cfg(feature = "map")]
mod views;
#[cfg(feature = "map")]
mod widgets;
