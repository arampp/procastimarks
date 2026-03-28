pub mod repository;
pub mod schema;

pub use repository::{BookmarkRepository, InsertResult};
pub use schema::init_db;
pub use schema::run_schema;
