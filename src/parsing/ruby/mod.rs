//! Ruby language parser module

pub mod audit;
pub mod behavior;
pub mod definition;
pub mod parser;
pub mod prototype;

pub use behavior::RubyBehavior;
pub use definition::RubyLanguage;
pub use parser::RubyParser;
pub use prototype::RubyParserPrototype;

// Re-export for registry registration
pub(crate) use definition::register;
