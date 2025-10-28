//! Generate Ruby grammar JSON file
//!
//! Run with: cargo test --test ruby_grammar_gen -- --nocapture

#[cfg(test)]
mod tests {
    use std::fs;
    use tree_sitter::Language;

    #[test]
    fn generate_ruby_grammar_json() {
        let language: Language = tree_sitter_ruby::LANGUAGE.into();

        let mut json = String::from("[\n");
        let mut first = true;

        for i in 0..language.node_kind_count() {
            if language.node_kind_is_named(i as u16) {
                let node_name = language.node_kind_for_id(i as u16).unwrap();
               
                if !first {
                    json.push_str(",\n");
                }
                first = false;
                
                json.push_str("  {\n");
                json.push_str(&format!("    \"type\": \"{}\",\n", node_name));
                json.push_str("    \"named\": true\n");
                json.push_str("  }");
            }
        }

        json.push_str("\n]\n");

        fs::create_dir_all("contributing/parsers/ruby")
            .expect("Failed to create Ruby parser directory");
        
        fs::write("contributing/parsers/ruby/grammar-node-types.json", &json)
            .expect("Failed to write grammar JSON");

        println!("✅ Generated Ruby grammar JSON with {} total node kinds", language.node_kind_count());
        println!("   File: contributing/parsers/ruby/grammar-node-types.json");
    }
}
