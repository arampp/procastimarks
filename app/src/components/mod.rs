/// UI component modules.
///
/// Each module contains one or more Leptos components that together implement
/// the application's user-facing views.
pub mod add_bookmark_form;
pub mod bookmark_list;
pub mod bookmarklet;

pub use add_bookmark_form::AddBookmarkPage;
pub use bookmark_list::BookmarkList;
pub use bookmarklet::BookmarkletInstall;
