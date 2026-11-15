//! Built-in desktop applications: terminal, text editor, and file browser.

pub mod edit;
pub mod files;
pub mod term;

pub use edit::Editor;
pub use files::Files;
pub use term::Terminal;
