// Instructions module re-exports
// STRUCTURAL REFACTOR: Mechanical move only, no behavioral changes

pub mod vault;
pub mod trader;
pub mod swap;
pub mod admin;

pub use vault::*;
pub use trader::*;
pub use swap::*;
pub use admin::*;
