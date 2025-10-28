//! Ruby Parser Comprehensive Analysis
//!
//! Generates:
//! - AUDIT_REPORT.md (symbol extraction coverage)
//! - GRAMMAR_ANALYSIS.md (grammar vs implementation analysis)
//! - node_discovery.txt (discovered nodes in examples)
//!
//! Run with: cargo test --test exploration_tests comprehensive_ruby_analysis -- --nocapture

#[cfg(test)]
mod tests {
    use codanna::io::format::format_utc_timestamp;
    use codanna::parsing::ruby::audit::RubyParserAudit;
    use serde_json::Value;
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use tree_sitter::{Language, Node, Parser};

    #[test]
    fn comprehensive_ruby_analysis() {
        println!("=== Ruby Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/ruby/grammar-node-types.json")
            .expect("Failed to read Ruby grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the parser audit on the comprehensive example
        let audit = match RubyParserAudit::audit_file("examples/ruby/comprehensive.rb") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit Ruby file: {e}");
                // Create empty audit for fallback
                RubyParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/ruby/AUDIT_REPORT.md", &report)
            .expect("Failed to write Ruby audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# Ruby Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", format_utc_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.rb: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.rb but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## 📝 Missing from Examples\n");
            analysis.push_str("These grammar nodes aren't in comprehensive.rb:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Add extracted symbol kinds info
        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## 🎯 Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write("contributing/parsers/ruby/GRAMMAR_ANALYSIS.md", &analysis)
            .expect("Failed to write Ruby grammar analysis");

        // 4. Generate node_discovery.txt
        let node_discovery = generate_ruby_node_discovery();
        fs::write(
            "contributing/parsers/ruby/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write Ruby node discovery");

        println!("📄 Ruby Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        if !example_nodes.is_empty() {
            println!(
                "  - Coverage: {:.1}%",
                audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
            );
        }
        println!("✅ Ruby documentation generated:");
        println!("   - contributing/parsers/ruby/AUDIT_REPORT.md");
        println!("   - contributing/parsers/ruby/GRAMMAR_ANALYSIS.md");
        println!("   - contributing/parsers/ruby/node_discovery.txt");
    }

    fn generate_ruby_node_discovery() -> String {
        let mut output = String::new();
        output.push_str("=== Ruby Language ABI-15 COMPREHENSIVE NODE MAPPING ===\n");
        output.push_str(&format!("  Generated: {}\n", format_utc_timestamp()));
        output.push_str("  ABI Version: 15\n");

        // Parse the comprehensive example
        let code = match fs::read_to_string("examples/ruby/comprehensive.rb") {
            Ok(code) => code,
            Err(e) => {
                output.push_str(&format!("\n⚠️ Error reading comprehensive.rb: {e}\n"));
                return output;
            }
        };

        let mut parser = Parser::new();
        let language: Language = tree_sitter_ruby::LANGUAGE.into();
        if let Err(e) = parser.set_language(&language) {
            output.push_str(&format!("\n⚠️ Error setting language: {e}\n"));
            return output;
        }

        let tree = match parser.parse(&code, None) {
            Some(tree) => tree,
            None => {
                output.push_str("\n⚠️ Failed to parse comprehensive.rb\n");
                return output;
            }
        };

        let mut node_registry = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(tree.root_node(), &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        // Define Ruby node categories for organization
        let node_categories = vec![
            (
                "MODULE AND CLASS NODES",
                vec![
                    "module",
                    "class",
                    "singleton_class",
                    "superclass",
                    "body_statement",
                ],
            ),
            (
                "METHOD DEFINITION NODES",
                vec![
                    "method",
                    "singleton_method",
                    "method_parameters",
                    "block_parameters",
                    "optional_parameter",
                    "keyword_parameter",
                    "hash_splat_parameter",
                    "splat_parameter",
                    "block_parameter",
                ],
            ),
            (
                "VARIABLE AND CONSTANT NODES",
                vec![
                    "assignment",
                    "operator_assignment",
                    "constant",
                    "instance_variable",
                    "class_variable",
                    "global_variable",
                    "identifier",
                ],
            ),
            (
                "BLOCK AND LAMBDA NODES",
                vec![
                    "block",
                    "do_block",
                    "lambda",
                    "begin_block",
                    "end_block",
                ],
            ),
            (
                "CONTROL FLOW NODES",
                vec![
                    "if",
                    "unless",
                    "case",
                    "when",
                    "else",
                    "elsif",
                    "if_modifier",
                    "unless_modifier",
                ],
            ),
            (
                "LOOP NODES",
                vec![
                    "while",
                    "until",
                    "for",
                    "while_modifier",
                    "until_modifier",
                    "break",
                    "next",
                    "redo",
                    "retry",
                ],
            ),
            (
                "EXCEPTION HANDLING NODES",
                vec!["begin", "rescue", "rescue_modifier", "ensure", "raise"],
            ),
            (
                "METHOD CALL NODES",
                vec![
                    "call",
                    "method_call",
                    "argument_list",
                    "block_argument",
                    "keyword_argument",
                ],
            ),
            (
                "SYMBOL AND STRING NODES",
                vec![
                    "symbol",
                    "string",
                    "interpolation",
                    "escape_sequence",
                    "heredoc_beginning",
                    "heredoc_body",
                    "heredoc_end",
                ],
            ),
            (
                "ATTRIBUTE NODES",
                vec!["attr_reader", "attr_writer", "attr_accessor"],
            ),
            (
                "ALIAS AND MODULE MANIPULATION",
                vec!["alias", "undef", "include", "extend", "prepend"],
            ),
        ];

        for (category, nodes_to_check) in &node_categories {
            output.push_str(&format!("=== {} ===\n", category));
            for node_name in nodes_to_check {
                if let Some(id) = node_registry.get(*node_name) {
                    output.push_str(&format!("  ✓ {:<35} -> ID: {}\n", node_name, id));
                } else {
                    output.push_str(&format!("  ✗ {:<35} NOT FOUND\n", node_name));
                }
            }
            output.push('\n');
        }

        // List all other nodes not categorized
        let categorized: HashSet<&str> = node_categories
            .iter()
            .flat_map(|(_, nodes)| nodes.iter().copied())
            .collect();

        let uncategorized: Vec<_> = node_registry
            .keys()
            .filter(|k| !categorized.contains(k.as_str()))
            .collect();

        if !uncategorized.is_empty() {
            output.push_str("=== UNCATEGORIZED NODES ===\n");
            let mut sorted_uncategorized = uncategorized;
            sorted_uncategorized.sort();
            for node_name in sorted_uncategorized {
                if let Some(id) = node_registry.get(node_name) {
                    output.push_str(&format!("  ✓ {:<35} -> ID: {}\n", node_name, id));
                }
            }
            output.push('\n');
        }

        output.push_str("Legend: ✓ = found in file, ○ = in grammar but not in file, ✗ = not in grammar\n");

        output
    }

    fn discover_nodes_with_ids(
        node: Node,
        registry: &mut HashMap<String, u16>,
        found_in_file: &mut HashSet<String>,
    ) {
        let kind = node.kind().to_string();
        registry.insert(kind.clone(), node.kind_id());
        found_in_file.insert(kind);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            discover_nodes_with_ids(child, registry, found_in_file);
        }
    }
}
