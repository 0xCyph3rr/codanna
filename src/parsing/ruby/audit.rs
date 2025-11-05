//! Ruby parser audit module
//!
//! Tracks which AST nodes the parser actually handles vs what's available in the grammar.
//! This helps identify gaps in our symbol extraction.

use super::RubyParser;
use crate::io::format::format_utc_timestamp;
use crate::parsing::NodeTracker;
use crate::types::FileId;
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tree_sitter::{Node, Parser};

#[derive(Error, Debug)]
pub enum AuditError {
    #[error("Failed to read file: {0}")]
    FileRead(#[from] std::io::Error),

    #[error("Failed to set language: {0}")]
    LanguageSetup(String),

    #[error("Failed to parse code")]
    ParseFailure,

    #[error("Failed to create parser: {0}")]
    ParserCreation(String),
}

pub struct RubyParserAudit {
    /// Nodes found in the grammar/file
    pub grammar_nodes: HashMap<String, u16>,
    /// Nodes our parser actually processes (from tracking parse calls)
    pub implemented_nodes: HashSet<String>,
    /// Symbols actually extracted
    pub extracted_symbol_kinds: HashSet<String>,
}

impl RubyParserAudit {
    /// Run audit on a Ruby source file
    pub fn audit_file(file_path: &str) -> Result<Self, AuditError> {
        let code = std::fs::read_to_string(file_path)?;
        Self::audit_code(&code)
    }

    /// Run audit on Ruby source code
    pub fn audit_code(code: &str) -> Result<Self, AuditError> {
        // First, discover all nodes in the file using tree-sitter directly
        let mut parser = Parser::new();
        let language = tree_sitter_ruby::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| AuditError::LanguageSetup(e.to_string()))?;

        let tree = parser.parse(code, None).ok_or(AuditError::ParseFailure)?;

        let mut grammar_nodes = HashMap::new();
        discover_nodes(tree.root_node(), &mut grammar_nodes);

        // Now parse with our actual parser to see what symbols get extracted
        let mut ruby_parser =
            RubyParser::new().map_err(|e| AuditError::ParserCreation(e.to_string()))?;
        let file_id = FileId(1);
        let mut symbol_counter = crate::types::SymbolCounter::new();
        let symbols = ruby_parser.parse(code, file_id, &mut symbol_counter);

        // Track which symbol kinds were produced
        let mut extracted_symbol_kinds = HashSet::new();
        for symbol in &symbols {
            extracted_symbol_kinds.insert(format!("{:?}", symbol.kind));
        }

        // Get dynamically tracked nodes from the parser (zero maintenance!)
        let implemented_nodes: HashSet<String> = ruby_parser
            .get_handled_nodes()
            .iter()
            .map(|handled_node| handled_node.name.clone())
            .collect();

        Ok(Self {
            grammar_nodes,
            implemented_nodes,
            extracted_symbol_kinds,
        })
    }

    /// Generate coverage report
    pub fn generate_report(&self) -> String {
        let mut report = String::new();

        report.push_str("# Ruby Parser Symbol Extraction Coverage Report\n\n");
        report.push_str(&format!("*Generated: {}*\n\n", format_utc_timestamp()));

        // Summary
        report.push_str("## Summary\n");
        report.push_str(&format!("- Nodes in file: {}\n", self.grammar_nodes.len()));
        report.push_str(&format!(
            "- Nodes with symbol extraction: {}\n",
            self.implemented_nodes.len()
        ));
        report.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            self.extracted_symbol_kinds.len()
        ));

        report.push_str("\n> **Note**: This report tracks nodes that produce indexed symbols for code intelligence.\n");
        report.push_str("> For complete grammar coverage, see GRAMMAR_ANALYSIS.md\n");

        // Coverage table for key symbol extraction nodes
        report.push_str("\n## Coverage Table\n\n");
        report.push_str("*Showing key nodes relevant for symbol extraction. Status determined by dynamic tracking.*\n\n");
        report.push_str("| Node Type | ID | Status |\n");
        report.push_str("|-----------|-----|--------|\n");

        // Key nodes relevant for Ruby symbol extraction
        let key_nodes = vec![
            // Module and class definitions
            "module",
            "class",
            "singleton_class",
            // Method definitions
            "method",
            "singleton_method",
            // Variables and constants
            "assignment",
            "operator_assignment",
            "constant",
            "instance_variable",
            "class_variable",
            "global_variable",
            // Blocks and lambdas
            "block",
            "lambda",
            "do_block",
            // Control structures
            "if",
            "unless",
            "case",
            "when",
            // Loops
            "while",
            "until",
            "for",
            // Exception handling
            "begin",
            "rescue",
            "ensure",
            // Method calls
            "call",
            "method_call",
            // Parameters
            "method_parameters",
            "block_parameters",
            "optional_parameter",
            "keyword_parameter",
            "splat_parameter",
            "block_parameter",
            // Symbols and strings
            "symbol",
            "string",
            "heredoc_beginning",
            // Attributes
            "attr_reader",
            "attr_writer",
            "attr_accessor",
            // Aliases
            "alias",
            "undef",
            // Modules
            "include",
            "extend",
            "prepend",
        ];

        let mut gaps = Vec::new();
        let mut missing = Vec::new();

        for node_name in key_nodes {
            let status = if let Some(id) = self.grammar_nodes.get(node_name) {
                if self.implemented_nodes.contains(node_name) {
                    format!("{id} | ✅ implemented")
                } else {
                    gaps.push(node_name);
                    format!("{id} | ⚠️ gap")
                }
            } else {
                missing.push(node_name);
                "- | ❌ not found".to_string()
            };
            report.push_str(&format!("| {node_name} | {status} |\n"));
        }

        // Add legend
        report.push_str("\n## Legend\n\n");
        report
            .push_str("- ✅ **implemented**: Node type is recognized and handled by the parser\n");
        report.push_str("- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)\n");
        report.push_str("- ❌ **not found**: Node type not present in the example file (may need better examples)\n");

        // Add recommendations
        report.push_str("\n## Recommended Actions\n\n");

        if !gaps.is_empty() {
            report.push_str("### Priority 1: Implementation Gaps\n");
            report.push_str("These nodes exist in your code but aren't being captured:\n\n");
            for gap in &gaps {
                report.push_str(&format!("- `{gap}`: Add parsing logic in parser.rs\n"));
            }
            report.push('\n');
        }

        if !missing.is_empty() {
            report.push_str("### Priority 2: Missing Examples\n");
            report.push_str("These nodes aren't in the comprehensive example. Consider:\n\n");
            for node in &missing {
                report.push_str(&format!(
                    "- `{node}`: Add example to comprehensive.rb or verify node name\n"
                ));
            }
            report.push('\n');
        }

        if gaps.is_empty() && missing.is_empty() {
            report.push_str("✨ **Excellent coverage!** All key nodes are implemented.\n");
        }

        report
    }
}

fn discover_nodes(node: Node, registry: &mut HashMap<String, u16>) {
    registry.insert(node.kind().to_string(), node.kind_id());

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        discover_nodes(child, registry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_simple_ruby() {
        let code = r#"
class Example
  def initialize
    @value = 42
  end

  def get_value
    @value
  end
end

def main
  example = Example.new
  puts example.get_value
end
"#;

        let audit = RubyParserAudit::audit_code(code).unwrap();

        // Should find these nodes in the code
        assert!(audit.grammar_nodes.contains_key("class"));
        assert!(audit.grammar_nodes.contains_key("method"));
        assert!(audit.grammar_nodes.contains_key("assignment"));

        // Should extract Class and Method symbols
        assert!(audit.extracted_symbol_kinds.contains("Class"));
        assert!(audit.extracted_symbol_kinds.contains("Method"));
    }
}
