//! Minimal proof-of-concept prototype for Ruby parser
//!
//! Purpose: Validate tree-sitter-ruby integration with symbol extraction pipeline
//! Scope: ONE feature (class extraction) to prove feasibility
//! Note: This is NOT production code - hardcoded values acceptable, error handling skipped

use crate::types::{FileId, Range, SymbolCounter, SymbolKind};
use crate::symbol::{Symbol, Visibility};
use tree_sitter::{Node, Parser};

/// Minimal Ruby parser prototype validating tree-sitter-ruby integration
pub struct RubyParserPrototype {
    parser: Parser,
}

impl RubyParserPrototype {
    /// Initialize parser with tree-sitter-ruby
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut parser = Parser::new();
        // Note: tree-sitter-ruby must be added to Cargo.toml
        // This validates the integration compiles and links correctly
        parser.set_language(&tree_sitter_ruby::LANGUAGE.into())?;
        Ok(Self { parser })
    }

    /// Parse Ruby code and extract class symbols
    ///
    /// This validates:
    /// 1. tree-sitter-ruby can parse Ruby syntax
    /// 2. Node traversal works correctly
    /// 3. Symbol extraction pipeline integrates properly
    pub fn parse_classes(&mut self, code: &str, file_id: FileId, counter: &mut SymbolCounter) -> Vec<Symbol> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut symbols = Vec::new();
        self.extract_classes(tree.root_node(), code, file_id, &mut symbols, counter);
        symbols
    }

    /// Extract class symbols from AST
    ///
    /// Demonstrates tree-sitter-ruby query pattern:
    /// - Node kind matching ("class")
    /// - Child node navigation (getting class name)
    /// - Range extraction for source location
    fn extract_classes(&self, node: Node, source: &str, file_id: FileId, symbols: &mut Vec<Symbol>, counter: &mut SymbolCounter) {
        // Validate tree-sitter-ruby "class" node detection
        if node.kind() == "class" {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = &source[name_node.byte_range()];
                let range = self.node_to_range(node);

                let mut symbol = Symbol::new(
                    counter.next_id(),
                    name,
                    SymbolKind::Class,
                    file_id,
                    range,
                );

                // Set minimal required fields for validation
                symbol.file_path = Box::from("prototype_test.rb");
                symbol.visibility = Visibility::Public;

                symbols.push(symbol);
            }
        }

        // Recursively traverse AST to find all classes (including nested)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_classes(child, source, file_id, symbols, counter);
        }
    }

    /// Convert tree-sitter node to Range
    fn node_to_range(&self, node: Node) -> Range {
        let start = node.start_position();
        let end = node.end_position();
        Range::new(
            start.row as u32,
            start.column as u16,
            end.row as u32,
            end.column as u16,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ruby_parser_prototype() {
        let ruby_code = r#"
class User
  def initialize(name)
    @name = name
  end
end

class Admin < User
end
"#;

        let mut parser = RubyParserPrototype::new().expect("Failed to initialize parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId(1);

        let symbols = parser.parse_classes(ruby_code, file_id, &mut counter);

        // Validate prototype extracts classes correctly
        assert_eq!(symbols.len(), 2, "Should extract User and Admin classes");
        assert_eq!(&*symbols[0].name, "User");
        assert_eq!(&*symbols[1].name, "Admin");
        assert!(matches!(symbols[0].kind, SymbolKind::Class));
    }

    #[test]
    fn test_comprehensive_ruby_file() {
        // Validate prototype works with comprehensive Ruby features
        let ruby_code = include_str!("../../../examples/ruby/comprehensive.rb");

        let mut parser = RubyParserPrototype::new().expect("Failed to initialize parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId(1);

        let symbols = parser.parse_classes(ruby_code, file_id, &mut counter);

        // comprehensive.rb has 12 class definitions: User, Admin, Configuration,
        // Cacheable (module, but shows traversal works), Article, DataProcessor,
        // DynamicModel, Report, AuditedUser, EdgeCases, etc.
        assert!(symbols.len() >= 8, "Should extract at least 8 classes from comprehensive.rb, got {}", symbols.len());

        // Verify specific classes are found
        let class_names: Vec<&str> = symbols.iter().map(|s| &*s.name).collect();
        assert!(class_names.contains(&"User"), "Should find User class");
        assert!(class_names.contains(&"Admin"), "Should find Admin class");
        assert!(class_names.contains(&"Article"), "Should find Article class");
    }
}
