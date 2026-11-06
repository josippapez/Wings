//! The Project page's data (`projects/<id>/info.json`) lives in the shared `wings-spaces` crate, and the tools an agent
//! reads and writes it with over MCP in `wings-connect`; the native app uses both too. This puts them back under the
//! names the rest of the app uses.

pub use wings_connect::project_info::*;
pub use wings_spaces::project_info::*;
