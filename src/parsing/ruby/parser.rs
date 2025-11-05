//! Ruby language parser implementation
//!
//! This parser provides Ruby language support for the codebase intelligence system.
//! It extracts symbols, relationships, and documentation from Ruby source code using
//! tree-sitter for AST parsing.
//!
//! **Tree-sitter ABI Version**: ABI-14 (tree-sitter-ruby 0.23.1)
//!
//! Note: This parser uses ABI-14 compatible with tree-sitter 0.25.8. The tree-sitter-ruby
//! grammar supports all required node types for Ruby parsing. When upgrading to a newer
//! tree-sitter-ruby version, verify compatibility with node type names used in this implementation.

use crate::parsing::Import;
use crate::parsing::context::Visibility as ContextVisibility;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Language, LanguageParser, MethodCall, NodeTracker, NodeTrackingState,
    ParserContext, ScopeType,
};
use crate::symbol::Visibility as SymbolVisibility;
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind};
use std::any::Any;
use thiserror::Error;
use tree_sitter::{Node, Parser};

/// Ruby-specific parsing errors
#[derive(Error, Debug)]
pub enum RubyParseError {
    #[error(
        "Failed to initialize Ruby parser: {reason}\nSuggestion: Ensure tree-sitter-ruby is properly installed and the version matches Cargo.toml"
    )]
    ParserInitFailed { reason: String },

    #[error(
        "Invalid Ruby syntax at {location:?}: {details}\nSuggestion: Check for missing 'end' keywords, incorrect indentation, or unclosed brackets/quotes"
    )]
    SyntaxError { location: Range, details: String },

    #[error(
        "Failed to parse type annotation: {annotation}\nSuggestion: Ensure type annotations follow Ruby 3+ syntax (e.g., sig {{ params(x: String).returns(Integer) }})"
    )]
    InvalidTypeAnnotation { annotation: String },
}

/// Ruby language parser
pub struct RubyParser {
    parser: Parser,
    node_tracker: NodeTrackingState,
}

impl std::fmt::Debug for RubyParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RubyParser")
            .field("language", &"Ruby")
            .finish()
    }
}

impl RubyParser {
    /// Create a new Ruby parser instance
    pub fn new() -> Result<Self, RubyParseError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_ruby::LANGUAGE.into())
            .map_err(|e| RubyParseError::ParserInitFailed {
                reason: format!("tree-sitter error: {e}"),
            })?;

        Ok(Self {
            parser,
            node_tracker: NodeTrackingState::new(),
        })
    }

    /// Parse Ruby source code and extract all symbols
    pub fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut symbols = Vec::new();
        // Create a parser context starting at module scope
        let mut context = ParserContext::new();

        // Create a module-level symbol to represent the file's module scope.
        // Name is set to "<module>" here to match Ruby conventions;
        // during indexing, RubyBehavior will rename it to the actual module path
        // (e.g., MyModule::MyClass) for searchability.
        let module_symbol_id = symbol_counter.next_id();
        let module_range = self.node_to_range(root_node);
        let mut module_symbol = Symbol::new(
            module_symbol_id,
            "<module>",
            SymbolKind::Module,
            file_id,
            module_range,
        );
        module_symbol.scope_context = Some(crate::symbol::ScopeContext::Module);
        symbols.push(module_symbol);

        self.extract_symbols_from_node(
            root_node,
            code,
            file_id,
            &mut symbols,
            symbol_counter,
            &mut context,
            0,
        );

        symbols
    }

    /// Extract symbols from AST node recursively
    fn extract_symbols_from_node(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        match node.kind() {
            "class" => {
                self.register_handled_node(node.kind(), node.kind_id());

                // Extract class symbol
                if let Some(class_symbol) =
                    self.process_class(node, code, file_id, counter, context)
                {
                    let class_name = class_symbol.name.to_string();
                    symbols.push(class_symbol);

                    // Enter class scope for nested members
                    context.set_current_class(Some(class_name));
                    context.enter_scope(ScopeType::Class);
                    // Reset visibility to public when entering a new class
                    context.reset_visibility();

                    // Process class body (methods, nested classes, etc.)
                    self.process_children(node, code, file_id, symbols, counter, context, depth);

                    // Exit class scope
                    context.exit_scope();
                    context.set_current_class(None);
                    // Reset visibility to public when exiting class
                    context.reset_visibility();
                }
            }
            "module" => {
                self.register_handled_node(node.kind(), node.kind_id());

                // Extract module symbol
                if let Some(module_symbol) =
                    self.process_module(node, code, file_id, counter, context)
                {
                    symbols.push(module_symbol);

                    // Enter module scope for nested members
                    // Note: Modules in Ruby can contain classes, methods, and other modules
                    context.enter_scope(ScopeType::Module);

                    // Process module body
                    self.process_children(node, code, file_id, symbols, counter, context, depth);

                    // Exit module scope
                    context.exit_scope();
                }
            }
            "method" => {
                self.register_handled_node(node.kind(), node.kind_id());

                // Extract method symbol with current visibility
                if let Some(method_symbol) =
                    self.process_method(node, code, file_id, counter, context)
                {
                    symbols.push(method_symbol);
                }

                // Process method body
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "singleton_method" => {
                self.register_handled_node(node.kind(), node.kind_id());

                // Extract singleton (class) method symbol
                if let Some(method_symbol) =
                    self.process_singleton_method(node, code, file_id, counter, context)
                {
                    symbols.push(method_symbol);
                }

                // Process method body
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "call" => {
                self.register_handled_node(node.kind(), node.kind_id());

                // Check for visibility modifiers and metaprogramming
                self.process_call_node(node, code, file_id, symbols, counter, context);

                // Process call arguments and children
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "assignment" => {
                self.register_handled_node(node.kind(), node.kind_id());

                // Extract constant if left side is uppercase identifier
                if let Some(constant_symbol) =
                    self.process_assignment(node, code, file_id, counter, context)
                {
                    symbols.push(constant_symbol);
                }

                // Process children to handle nested structures
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "identifier" => {
                // Handle standalone identifiers that might be visibility modifiers
                if context.is_in_class() {
                    let id_text = &code[node.byte_range()];
                    match id_text {
                        "private" => {
                            context.set_visibility(ContextVisibility::Private);
                            return;
                        }
                        "protected" => {
                            context.set_visibility(ContextVisibility::Protected);
                            return;
                        }
                        "public" => {
                            context.set_visibility(ContextVisibility::Public);
                            return;
                        }
                        _ => {}
                    }
                }
                // Process children for other identifiers
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            _ => {
                // Recursively process children for unhandled node types
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
        }
    }

    /// Process all child nodes recursively
    fn process_children(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        depth: usize,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols_from_node(
                child,
                code,
                file_id,
                symbols,
                counter,
                context,
                depth + 1,
            );
        }
    }

    /// Convert tree-sitter Node to Range
    fn node_to_range(&self, node: Node) -> Range {
        let start_pos = node.start_position();
        let end_pos = node.end_position();
        Range {
            start_line: start_pos.row as u32,
            start_column: start_pos.column as u16,
            end_line: end_pos.row as u32,
            end_column: end_pos.column as u16,
        }
    }

    /// Process a class definition node
    ///
    /// Extracts class name, superclass (if present), and creates a Class symbol.
    /// Follows the Python parser pattern (src/parsing/python/parser.rs:341-368).
    fn process_class(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) -> Option<Symbol> {
        // Extract class name from 'name' field
        let class_name = self.extract_class_name(node, code)?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        // Create class symbol
        let mut symbol = Symbol::new(symbol_id, class_name, SymbolKind::Class, file_id, range);

        // Set scope context based on current parser context
        symbol.scope_context = Some(context.current_scope_context());

        // Generate signature: "class ClassName" or "class ClassName < ParentClass"
        let signature = self.extract_class_signature(node, code, class_name);
        symbol.signature = Some(signature.into());

        // Classes are public by default in Ruby
        symbol.visibility = SymbolVisibility::Public;

        // Extract documentation comment
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(|s| s.into());

        Some(symbol)
    }

    /// Process a module definition node
    ///
    /// Extracts module name and creates a Module symbol.
    fn process_module(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) -> Option<Symbol> {
        // Extract module name from 'name' field
        let module_name = self.extract_module_name(node, code)?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        // Create module symbol
        let mut symbol = Symbol::new(symbol_id, module_name, SymbolKind::Module, file_id, range);

        // Set scope context based on current parser context
        symbol.scope_context = Some(context.current_scope_context());

        // Generate signature: "module ModuleName"
        symbol.signature = Some(format!("module {module_name}").into());

        // Modules are public by default in Ruby
        symbol.visibility = SymbolVisibility::Public;

        // Extract documentation comment
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(|s| s.into());

        Some(symbol)
    }

    /// Extract class name from class definition node
    ///
    /// Tree-sitter-ruby AST: class nodes have a 'name' field containing the class identifier
    fn extract_class_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        node.child_by_field_name("name")
            .map(|name_node| &code[name_node.byte_range()])
    }

    /// Extract module name from module definition node
    ///
    /// Tree-sitter-ruby AST: module nodes have a 'name' field containing the module identifier
    fn extract_module_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        node.child_by_field_name("name")
            .map(|name_node| &code[name_node.byte_range()])
    }

    /// Extract class signature including superclass if present
    ///
    /// Generates:
    /// - "class User" (no inheritance)
    /// - "class Admin < User" (with inheritance)
    ///
    /// Tree-sitter-ruby AST: class nodes have optional 'superclass' field
    /// Note: The superclass field includes the '<' character (e.g., "< User")
    fn extract_class_signature(&self, node: Node, code: &str, class_name: &str) -> String {
        // Check for superclass field
        if let Some(superclass_node) = node.child_by_field_name("superclass") {
            let superclass_text = &code[superclass_node.byte_range()];
            // The superclass field includes "< ParentClass", so we extract just the parent name
            // by trimming the '<' and whitespace
            let superclass_name = superclass_text.trim_start_matches('<').trim();
            format!("class {class_name} < {superclass_name}")
        } else {
            format!("class {class_name}")
        }
    }

    /// Process an instance method definition node
    ///
    /// Extracts method name, parameters, and creates a Method symbol with proper visibility.
    /// Follows the Python parser's process_function pattern.
    fn process_method(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) -> Option<Symbol> {
        // Extract method name from 'name' field
        let method_name = self.extract_method_name(node, code)?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        // Create method symbol
        let mut symbol = Symbol::new(symbol_id, method_name, SymbolKind::Method, file_id, range);

        // Set scope context based on current parser context
        symbol.scope_context = Some(context.current_scope_context());

        // Generate method signature with parameters
        let signature = self.extract_method_signature(node, code, method_name, false);
        symbol.signature = Some(signature.into());

        // Set visibility based on current context
        symbol.visibility = Self::convert_visibility(context.current_visibility());

        // Extract documentation comment
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(|s| s.into());

        Some(symbol)
    }

    /// Process a singleton (class) method definition node
    ///
    /// Extracts class method name, parameters, and creates a Method symbol.
    /// Ruby singleton methods are defined with `def self.method_name` or `def ClassName.method_name`.
    fn process_singleton_method(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) -> Option<Symbol> {
        // Extract method name from 'name' field
        let method_name = self.extract_method_name(node, code)?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        // Create method symbol
        let mut symbol = Symbol::new(symbol_id, method_name, SymbolKind::Method, file_id, range);

        // Set scope context based on current parser context
        symbol.scope_context = Some(context.current_scope_context());

        // Generate method signature with 'self.' prefix for class methods
        let signature = self.extract_method_signature(node, code, method_name, true);
        symbol.signature = Some(signature.into());

        // Singleton methods are always public in Ruby unless explicitly marked
        symbol.visibility = SymbolVisibility::Public;

        // Extract documentation comment
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(|s| s.into());

        Some(symbol)
    }

    /// Extract method name from method or singleton_method node
    ///
    /// Tree-sitter-ruby AST: method nodes have a 'name' field containing the method identifier
    fn extract_method_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        node.child_by_field_name("name")
            .map(|name_node| &code[name_node.byte_range()])
    }

    /// Extract method signature including parameters
    ///
    /// Generates:
    /// - "def method_name" (no parameters)
    /// - "def method_name(param1, param2)" (with parameters)
    /// - "def method_name(param1, param2 = default)" (with default values)
    /// - "def self.method_name(param1)" (singleton method)
    fn extract_method_signature(
        &self,
        node: Node,
        code: &str,
        method_name: &str,
        is_singleton: bool,
    ) -> String {
        let params = self.extract_parameters(node, code);

        if is_singleton {
            if params.is_empty() {
                format!("def self.{method_name}")
            } else {
                format!("def self.{method_name}({})", params.join(", "))
            }
        } else {
            if params.is_empty() {
                format!("def {method_name}")
            } else {
                format!("def {method_name}({})", params.join(", "))
            }
        }
    }

    /// Extract method parameters
    ///
    /// Handles:
    /// - Regular parameters: `param`
    /// - Optional parameters: `param = default`
    /// - Keyword arguments: `required:`, `optional: default`
    /// - Splat operator: `*args`
    /// - Double splat: `**kwargs`
    /// - Block parameter: `&block`
    fn extract_parameters(&self, node: Node, code: &str) -> Vec<String> {
        let mut params = Vec::new();

        // Find the 'parameters' field
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                match child.kind() {
                    "identifier" => {
                        // Simple parameter
                        params.push(code[child.byte_range()].to_string());
                    }
                    "optional_parameter" => {
                        // Parameter with default value: `param = default`
                        let param_text = &code[child.byte_range()];
                        params.push(param_text.to_string());
                    }
                    "keyword_parameter" => {
                        // Keyword argument: `key:` or `key: value`
                        let param_text = &code[child.byte_range()];
                        params.push(param_text.to_string());
                    }
                    "splat_parameter" => {
                        // Splat operator: `*args`
                        let param_text = &code[child.byte_range()];
                        params.push(param_text.to_string());
                    }
                    "hash_splat_parameter" => {
                        // Double splat: `**kwargs`
                        let param_text = &code[child.byte_range()];
                        params.push(param_text.to_string());
                    }
                    "block_parameter" => {
                        // Block parameter: `&block`
                        let param_text = &code[child.byte_range()];
                        params.push(param_text.to_string());
                    }
                    _ => {}
                }
            }
        }

        params
    }

    /// Process call nodes for visibility modifiers and metaprogramming
    ///
    /// Handles:
    /// - Visibility modifiers: `private`, `protected`, `public`
    /// - attr_accessor: generates getter and setter methods
    /// - attr_reader: generates getter method
    /// - attr_writer: generates setter method
    fn process_call_node(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
    ) {
        // Extract the method being called
        if let Some(method_node) = node.child_by_field_name("method") {
            let method_text = &code[method_node.byte_range()];

            // Handle visibility modifiers
            match method_text {
                "private" => {
                    context.set_visibility(ContextVisibility::Private);
                    return;
                }
                "protected" => {
                    context.set_visibility(ContextVisibility::Protected);
                    return;
                }
                "public" => {
                    context.set_visibility(ContextVisibility::Public);
                    return;
                }
                _ => {}
            }

            // Handle attr_accessor, attr_reader, attr_writer
            match method_text {
                "attr_accessor" | "attr_reader" | "attr_writer" => {
                    self.generate_synthetic_methods(
                        node,
                        code,
                        file_id,
                        symbols,
                        counter,
                        context,
                        method_text,
                    );
                }
                _ => {}
            }
        }
    }

    /// Generate synthetic getter/setter methods from attr_* calls
    ///
    /// - attr_reader :name → generates `def name`
    /// - attr_writer :name → generates `def name=(value)`
    /// - attr_accessor :name → generates both getter and setter
    fn generate_synthetic_methods(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &ParserContext,
        attr_type: &str,
    ) {
        // Find argument_list node containing the symbols
        if let Some(args_node) = node.child_by_field_name("arguments") {
            let mut cursor = args_node.walk();
            for child in args_node.children(&mut cursor) {
                // Extract simple_symbol nodes (e.g., :name, :email)
                if child.kind() == "simple_symbol" {
                    let symbol_text = &code[child.byte_range()];
                    // Remove the leading ':' from the symbol
                    let attr_name = symbol_text.trim_start_matches(':');

                    let range = self.node_to_range(child);

                    // Generate getter method if attr_reader or attr_accessor
                    if attr_type == "attr_reader" || attr_type == "attr_accessor" {
                        let getter_id = counter.next_id();
                        let mut getter = Symbol::new(
                            getter_id,
                            attr_name,
                            SymbolKind::Method,
                            file_id,
                            range.clone(),
                        );
                        getter.scope_context = Some(context.current_scope_context());
                        getter.signature = Some(format!("def {attr_name}").into());
                        getter.visibility = SymbolVisibility::Public;
                        symbols.push(getter);
                    }

                    // Generate setter method if attr_writer or attr_accessor
                    if attr_type == "attr_writer" || attr_type == "attr_accessor" {
                        let setter_id = counter.next_id();
                        let setter_name = format!("{attr_name}=");
                        let mut setter = Symbol::new(
                            setter_id,
                            setter_name,
                            SymbolKind::Method,
                            file_id,
                            range.clone(),
                        );
                        setter.scope_context = Some(context.current_scope_context());
                        setter.signature = Some(format!("def {attr_name}=(value)").into());
                        setter.visibility = SymbolVisibility::Public;
                        symbols.push(setter);
                    }
                }
            }
        }
    }

    /// Convert parsing context visibility to symbol visibility
    ///
    /// Ruby uses Public/Private/Protected, mapping to symbol visibility as:
    /// - Public -> Public
    /// - Private -> Private
    /// - Protected -> Module (closest equivalent in Rust-style visibility)
    fn convert_visibility(context_vis: ContextVisibility) -> SymbolVisibility {
        match context_vis {
            ContextVisibility::Public => SymbolVisibility::Public,
            ContextVisibility::Private => SymbolVisibility::Private,
            ContextVisibility::Protected => SymbolVisibility::Module,
        }
    }

    /// Process assignment nodes to extract constants
    ///
    /// Phase 4: Extracts constants (uppercase identifiers) at class/module scope.
    /// Ruby constants are identified by starting with uppercase letters.
    ///
    /// Examples:
    /// - VERSION = "1.0.0"
    /// - MAX_LOGIN_ATTEMPTS = 3
    /// - PERMISSIONS = ["read", "write", "delete"]
    ///
    /// Variables (@instance, @@class, $global) are handled pragmatically:
    /// - Not extracted as separate Symbol entries (not primary symbols in Codanna)
    /// - Could be documented in containing class/module doc_comment if needed
    /// - 80% coverage is acceptable - focus on constants which are searchable
    fn process_assignment(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) -> Option<Symbol> {
        // Get the left-hand side of the assignment
        let left_node = node.child_by_field_name("left")?;
        let left_text = &code[left_node.byte_range()];

        // Check if it's a constant (starts with uppercase letter)
        // Ruby constants must start with A-Z
        if !left_text.chars().next()?.is_ascii_uppercase() {
            // Not a constant - could be a variable (@var, @@var, $var) or regular identifier
            // Per Phase 4 guidance: variables are NOT primary symbols, skip extraction
            return None;
        }

        // Extract constant name
        let constant_name = left_text;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        // Create constant symbol
        let mut symbol = Symbol::new(
            symbol_id,
            constant_name,
            SymbolKind::Constant,
            file_id,
            range,
        );

        // Set scope context based on current parser context
        symbol.scope_context = Some(context.current_scope_context());

        // Generate signature: "CONSTANT_NAME = value" (extract right side if simple)
        let signature = self.extract_constant_signature(node, code, constant_name);
        symbol.signature = Some(signature.into());

        // Constants are always public in Ruby
        symbol.visibility = SymbolVisibility::Public;

        Some(symbol)
    }

    /// Extract constant signature including value if simple
    ///
    /// Generates:
    /// - "VERSION = \"1.0.0\"" (string literal)
    /// - "MAX_ATTEMPTS = 3" (integer literal)
    /// - "PERMISSIONS = [...]" (array - show [...])
    /// - "CONFIG = {...}" (hash - show {...})
    fn extract_constant_signature(&self, node: Node, code: &str, constant_name: &str) -> String {
        // Get the right-hand side of the assignment
        if let Some(right_node) = node.child_by_field_name("right") {
            let right_text = &code[right_node.byte_range()];

            // For simple literals, include the value
            // For complex structures (arrays, hashes), use placeholder
            match right_node.kind() {
                "string" | "integer" | "float" | "true" | "false" | "nil" | "symbol" => {
                    // Limit length to avoid overly long signatures
                    if right_text.len() <= 50 {
                        return format!("{} = {}", constant_name, right_text);
                    } else {
                        return format!("{} = {}...", constant_name, &right_text[..47]);
                    }
                }
                "array" => {
                    return format!("{} = [...]", constant_name);
                }
                "hash" => {
                    return format!("{} = {{{{...}}}}", constant_name);
                }
                _ => {
                    // For other expressions, just show the constant name
                    return format!("{} = <expression>", constant_name);
                }
            }
        }

        // Fallback if no right side found
        constant_name.to_string()
    }

    /// Find function calls in AST node recursively
    fn find_calls_in_node<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: &mut Option<&'a str>,
    ) {
        match node.kind() {
            "method" | "singleton_method" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(name) = self.extract_method_name(node, code) {
                    let old_function = *current_function;
                    *current_function = Some(name);
                    self.process_children_for_calls(node, code, calls, current_function);
                    *current_function = old_function;
                }
            }
            "call" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(target) = self.extract_call_target(node, code) {
                    let caller = (*current_function).unwrap_or("<module>");
                    let range = self.node_to_range(node);
                    calls.push((caller, target, range));
                }
                self.process_children_for_calls(node, code, calls, current_function);
            }
            _ => {
                self.process_children_for_calls(node, code, calls, current_function);
            }
        }
    }

    /// Process child nodes for function calls
    fn process_children_for_calls<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: &mut Option<&'a str>,
    ) {
        for child in node.children(&mut node.walk()) {
            self.find_calls_in_node(child, code, calls, current_function);
        }
    }

    /// Extract the target of a function call from Ruby call node
    fn extract_call_target<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        // Ruby call nodes have 'method' field for the method name
        let method_node = node.child_by_field_name("method")?;
        Some(&code[method_node.byte_range()])
    }

    /// Find method calls in AST node recursively
    fn find_method_calls_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        method_calls: &mut Vec<MethodCall>,
        current_function: &mut Option<&'a str>,
    ) {
        match node.kind() {
            "method" | "singleton_method" => {
                if let Some(name) = self.extract_method_name(node, code) {
                    let old_function = *current_function;
                    *current_function = Some(name);
                    self.process_children_for_method_calls(
                        node,
                        code,
                        method_calls,
                        current_function,
                    );
                    *current_function = old_function;
                }
            }
            "call" => {
                let caller = (*current_function).unwrap_or("<module>");
                if let Some(method_call) = self.extract_ruby_method_call(node, code, caller) {
                    method_calls.push(method_call);
                }
                self.process_children_for_method_calls(node, code, method_calls, current_function);
            }
            _ => {
                self.process_children_for_method_calls(node, code, method_calls, current_function);
            }
        }
    }

    /// Process child nodes for method calls
    fn process_children_for_method_calls<'a>(
        &self,
        node: Node,
        code: &'a str,
        method_calls: &mut Vec<MethodCall>,
        current_function: &mut Option<&'a str>,
    ) {
        for child in node.children(&mut node.walk()) {
            self.find_method_calls_in_node(child, code, method_calls, current_function);
        }
    }

    /// Extract method call from Ruby call node
    fn extract_ruby_method_call<'a>(
        &self,
        node: Node,
        code: &'a str,
        caller: &'a str,
    ) -> Option<MethodCall> {
        // Ruby call nodes have 'method' field for the method name
        let method_node = node.child_by_field_name("method")?;
        let method_name = &code[method_node.byte_range()];
        let range = self.node_to_range(node);

        // Extract receiver if present (obj.method syntax)
        let receiver = node
            .child_by_field_name("receiver")
            .map(|r| &code[r.byte_range()]);

        let mut method_call = MethodCall::new(caller, method_name, range);
        if let Some(recv) = receiver {
            method_call = method_call.with_receiver(recv);
        }

        Some(method_call)
    }

    /// Find constant/module uses in the AST (e.g., ConstantName.method_call patterns)
    fn find_constant_uses_in_node<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: &mut Option<&'a str>,
    ) {
        match node.kind() {
            "method" | "singleton_method" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(name) = self.extract_method_name(node, code) {
                    let old_function = *current_function;
                    *current_function = Some(name);
                    self.process_children_for_constant_uses(node, code, uses, current_function);
                    *current_function = old_function;
                }
            }
            "call" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Extract receiver from call nodes (e.g., User.find → "User")
                if let Some(receiver_node) = node.child_by_field_name("receiver") {
                    let receiver_text = &code[receiver_node.byte_range()];

                    // Check if receiver is a constant (starts with uppercase)
                    if let Some(first_char) = receiver_text.chars().next() {
                        if first_char.is_ascii_uppercase() {
                            let caller = (*current_function).unwrap_or("<module>");
                            let range = self.node_to_range(receiver_node);
                            uses.push((caller, receiver_text, range));
                        }
                    }
                }
                self.process_children_for_constant_uses(node, code, uses, current_function);
            }
            "scope_resolution" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Handle Module::Class patterns - extract the left side (module/constant name)
                if let Some(scope_node) = node.child_by_field_name("scope") {
                    let scope_text = &code[scope_node.byte_range()];

                    // Check if scope is a constant
                    if let Some(first_char) = scope_text.chars().next() {
                        if first_char.is_ascii_uppercase() {
                            let caller = (*current_function).unwrap_or("<module>");
                            let range = self.node_to_range(scope_node);
                            uses.push((caller, scope_text, range));
                        }
                    }
                }
                self.process_children_for_constant_uses(node, code, uses, current_function);
            }
            _ => {
                self.process_children_for_constant_uses(node, code, uses, current_function);
            }
        }
    }

    /// Process child nodes for constant uses
    fn process_children_for_constant_uses<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: &mut Option<&'a str>,
    ) {
        for child in node.children(&mut node.walk()) {
            self.find_constant_uses_in_node(child, code, uses, current_function);
        }
    }

    /// Recursively find mixin calls (include/extend/prepend) in the AST
    ///
    /// Handles:
    /// - include: Adds instance methods from a module
    /// - extend: Adds class methods from a module
    /// - prepend: Adds methods with precedence over include
    ///
    /// Mixins are NOT distinct node types in tree-sitter-ruby - they are 'call' nodes
    /// with identifier children having names 'include'/'extend'/'prepend'.
    fn find_mixins_in_node<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
        class_stack: &mut Vec<&'a str>,
    ) {
        match node.kind() {
            "class" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(class_name) = self.extract_class_name(node, code) {
                    class_stack.push(class_name);
                    self.process_children_for_mixins(node, code, implementations, class_stack);
                    class_stack.pop();
                }
            }
            "module" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(module_name) = self.extract_module_name(node, code) {
                    class_stack.push(module_name);
                    self.process_children_for_mixins(node, code, implementations, class_stack);
                    class_stack.pop();
                }
            }
            "singleton_class" => {
                // Singleton classes preserve parent context - don't push/pop stack
                self.register_handled_node(node.kind(), node.kind_id());
                self.process_children_for_mixins(node, code, implementations, class_stack);
            }
            "call" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Check if this is a mixin call (include/extend/prepend)
                if let Some(method_node) = node.child_by_field_name("method") {
                    let method_text = &code[method_node.byte_range()];

                    if method_text == "include"
                        || method_text == "extend"
                        || method_text == "prepend"
                    {
                        // Get the current class/module context
                        if let Some(implementer) = class_stack.last() {
                            // Extract module names from the argument list
                            self.extract_mixin_modules(node, code, implementer, implementations);
                        }
                    }
                }
                self.process_children_for_mixins(node, code, implementations, class_stack);
            }
            _ => {
                self.process_children_for_mixins(node, code, implementations, class_stack);
            }
        }
    }

    /// Extract module names from mixin call arguments
    ///
    /// Handles:
    /// - Simple constants: `include Enumerable`
    /// - Qualified names: `include Features::Security`
    /// - Multiple mixins: `include A, B, C`
    fn extract_mixin_modules<'a>(
        &self,
        call_node: Node,
        code: &'a str,
        implementer: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        if let Some(args_node) = call_node.child_by_field_name("arguments") {
            let mut cursor = args_node.walk();
            for child in args_node.children(&mut cursor) {
                match child.kind() {
                    "constant" | "scope_resolution" => {
                        // Extract module name using byte_range to handle both simple and qualified names
                        let module_name = &code[child.byte_range()];
                        let range = self.node_to_range(child);
                        implementations.push((implementer, module_name, range));
                    }
                    _ => {
                        // Skip other nodes (e.g., commas, parentheses, inline module definitions)
                    }
                }
            }
        }
    }

    /// Process child nodes for mixin extraction
    fn process_children_for_mixins<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
        class_stack: &mut Vec<&'a str>,
    ) {
        for child in node.children(&mut node.walk()) {
            self.find_mixins_in_node(child, code, implementations, class_stack);
        }
    }
}

impl NodeTracker for RubyParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}

impl LanguageParser for RubyParser {
    fn parse(&mut self, code: &str, file_id: FileId, counter: &mut SymbolCounter) -> Vec<Symbol> {
        Self::parse(self, code, file_id, counter)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn language(&self) -> Language {
        Language::Ruby
    }

    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        // Look for Ruby documentation comments (# or =begin...=end)
        // Collect consecutive comment nodes before the symbol definition
        let mut comments = Vec::new();
        let mut current = node.prev_sibling();

        // Traverse backwards through previous siblings to collect comments
        while let Some(prev) = current {
            if prev.kind() == "comment" {
                let comment_text = &code[prev.byte_range()];
                comments.push(comment_text);
                current = prev.prev_sibling();
            } else {
                // Stop at first non-comment node
                break;
            }
        }

        if comments.is_empty() {
            return None;
        }

        // Reverse to get original order (we collected backwards)
        comments.reverse();

        // Clean and join comments
        let cleaned_lines: Vec<String> = comments
            .iter()
            .flat_map(|comment| {
                // Handle =begin...=end multi-line comments
                if comment.starts_with("=begin") {
                    comment
                        .trim_start_matches("=begin")
                        .trim_end_matches("=end")
                        .lines()
                        .map(|line| line.trim().to_string())
                        .collect::<Vec<_>>()
                } else {
                    // Handle # single-line comments
                    comment
                        .lines()
                        .map(|line| line.trim_start_matches('#').trim().to_string())
                        .collect::<Vec<_>>()
                }
            })
            .collect();

        let result = cleaned_lines.join("\n").trim().to_string();
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut calls = Vec::new();
        let mut current_function = None;

        self.find_calls_in_node(root_node, code, &mut calls, &mut current_function);
        calls
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut method_calls = Vec::new();
        let mut current_function = None;

        self.find_method_calls_in_node(root_node, code, &mut method_calls, &mut current_function);
        method_calls
    }

    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root = tree.root_node();
        let mut implementations = Vec::new();
        let mut class_stack: Vec<&'a str> = Vec::new();

        self.find_mixins_in_node(root, code, &mut implementations, &mut class_stack);
        implementations
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root = tree.root_node();
        let mut uses = Vec::new();
        let mut current_function = None;

        self.find_constant_uses_in_node(root, code, &mut uses, &mut current_function);

        uses
    }

    fn find_defines<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // TODO: Phase 2 - Method definitions in classes/modules
        Vec::new()
    }

    fn find_imports(&mut self, _code: &str, _file_id: FileId) -> Vec<Import> {
        // TODO: Phase 2 - Ruby uses require/require_relative
        Vec::new()
    }
}

impl Default for RubyParser {
    fn default() -> Self {
        Self::new().expect("Failed to create default Ruby parser")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ruby_parser_creation() {
        let parser = RubyParser::new();
        assert!(parser.is_ok(), "Should create Ruby parser successfully");
    }

    #[test]
    fn test_ruby_parser_basic_structure() {
        let code = r#"
class User
  def initialize(name)
    @name = name
  end
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Should have module symbol + User class
        assert!(
            symbols.len() >= 2,
            "Should have at least module and User class"
        );
        assert_eq!(
            symbols[0].name.as_ref(),
            "<module>",
            "First symbol should be module"
        );

        // Find the User class
        let user_class = symbols.iter().find(|s| s.name.as_ref() == "User");
        assert!(user_class.is_some(), "Should find User class");

        let user_class = user_class.unwrap();
        assert_eq!(user_class.kind, SymbolKind::Class);
        assert_eq!(
            user_class.signature.as_ref().map(|s| s.as_ref()),
            Some("class User")
        );
    }

    #[test]
    fn test_class_with_inheritance() {
        let code = r#"
class User
end

class Admin < User
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find User and Admin classes
        let user_class = symbols.iter().find(|s| s.name.as_ref() == "User");
        let admin_class = symbols.iter().find(|s| s.name.as_ref() == "Admin");

        assert!(user_class.is_some(), "Should find User class");
        assert!(admin_class.is_some(), "Should find Admin class");

        let user_class = user_class.unwrap();
        let admin_class = admin_class.unwrap();

        // Verify User class has no superclass
        assert_eq!(
            user_class.signature.as_ref().map(|s| s.as_ref()),
            Some("class User")
        );

        // Verify Admin class has User as superclass
        assert_eq!(
            admin_class.signature.as_ref().map(|s| s.as_ref()),
            Some("class Admin < User")
        );
    }

    #[test]
    fn test_module_extraction() {
        let code = r#"
module Authentication
  VERSION = "1.0.0"

  def self.enabled?
    true
  end
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find Authentication module
        let auth_module = symbols.iter().find(|s| s.name.as_ref() == "Authentication");
        assert!(auth_module.is_some(), "Should find Authentication module");

        let auth_module = auth_module.unwrap();
        assert_eq!(auth_module.kind, SymbolKind::Module);
        assert_eq!(
            auth_module.signature.as_ref().map(|s| s.as_ref()),
            Some("module Authentication")
        );
    }

    #[test]
    fn test_nested_module() {
        let code = r#"
module Authentication
  module OAuth
    PROVIDER = "github"
  end
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find both modules
        let auth_module = symbols.iter().find(|s| s.name.as_ref() == "Authentication");
        let oauth_module = symbols.iter().find(|s| s.name.as_ref() == "OAuth");

        assert!(auth_module.is_some(), "Should find Authentication module");
        assert!(oauth_module.is_some(), "Should find OAuth module");

        assert_eq!(auth_module.unwrap().kind, SymbolKind::Module);
        assert_eq!(oauth_module.unwrap().kind, SymbolKind::Module);
    }

    #[test]
    fn test_class_and_module_mixed() {
        let code = r#"
module Authentication
  class User
  end
end

class Admin < Authentication::User
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Verify all symbols are extracted
        let auth_module = symbols.iter().find(|s| s.name.as_ref() == "Authentication");
        let user_class = symbols.iter().find(|s| s.name.as_ref() == "User");
        let admin_class = symbols.iter().find(|s| s.name.as_ref() == "Admin");

        assert!(auth_module.is_some(), "Should find Authentication module");
        assert!(user_class.is_some(), "Should find User class");
        assert!(admin_class.is_some(), "Should find Admin class");

        // Verify Admin inherits from Authentication::User (full qualified name in superclass)
        let admin_class = admin_class.unwrap();
        assert_eq!(
            admin_class.signature.as_ref().map(|s| s.as_ref()),
            Some("class Admin < Authentication::User")
        );
    }

    #[test]
    fn test_language_metadata() {
        let parser = RubyParser::new().expect("Failed to create parser");
        assert_eq!(parser.language(), Language::Ruby);
    }

    #[test]
    fn test_node_to_range() {
        let mut parser = RubyParser::new().expect("Failed to create parser");
        let code = "class Foo\nend";

        let tree = parser.parser.parse(code, None).expect("Failed to parse");
        let root = tree.root_node();

        let range = parser.node_to_range(root);
        assert_eq!(range.start_line, 0);
        assert_eq!(range.start_column, 0);
    }

    // Phase 3 tests - Method extraction

    #[test]
    fn test_instance_method_extraction() {
        let code = r#"
class User
  def initialize(name)
    @name = name
  end

  def greet
    "Hello, #{@name}"
  end
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find instance methods
        let initialize = symbols.iter().find(|s| s.name.as_ref() == "initialize");
        let greet = symbols.iter().find(|s| s.name.as_ref() == "greet");

        assert!(initialize.is_some(), "Should find initialize method");
        assert!(greet.is_some(), "Should find greet method");

        let initialize = initialize.unwrap();
        assert_eq!(initialize.kind, SymbolKind::Method);
        assert_eq!(
            initialize.signature.as_ref().map(|s| s.as_ref()),
            Some("def initialize(name)")
        );

        let greet = greet.unwrap();
        assert_eq!(greet.kind, SymbolKind::Method);
        assert_eq!(
            greet.signature.as_ref().map(|s| s.as_ref()),
            Some("def greet")
        );
    }

    #[test]
    fn test_class_method_extraction() {
        let code = r#"
class User
  def self.find(id)
    new(id)
  end

  def self.count
    @@count
  end
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find class methods
        let find = symbols.iter().find(|s| s.name.as_ref() == "find");
        let count = symbols.iter().find(|s| s.name.as_ref() == "count");

        assert!(find.is_some(), "Should find class method find");
        assert!(count.is_some(), "Should find class method count");

        let find = find.unwrap();
        assert_eq!(find.kind, SymbolKind::Method);
        assert_eq!(
            find.signature.as_ref().map(|s| s.as_ref()),
            Some("def self.find(id)")
        );

        let count = count.unwrap();
        assert_eq!(count.kind, SymbolKind::Method);
        assert_eq!(
            count.signature.as_ref().map(|s| s.as_ref()),
            Some("def self.count")
        );
    }

    #[test]
    fn test_method_visibility() {
        let code = r#"
class User
  def public_method
  end

  private

  def private_method
  end

  protected

  def protected_method
  end

  public

  def another_public_method
  end
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find methods with different visibility
        let public_method = symbols.iter().find(|s| s.name.as_ref() == "public_method");
        let private_method = symbols.iter().find(|s| s.name.as_ref() == "private_method");
        let protected_method = symbols
            .iter()
            .find(|s| s.name.as_ref() == "protected_method");
        let another_public = symbols
            .iter()
            .find(|s| s.name.as_ref() == "another_public_method");

        assert!(public_method.is_some(), "Should find public_method");
        assert!(private_method.is_some(), "Should find private_method");
        assert!(protected_method.is_some(), "Should find protected_method");
        assert!(
            another_public.is_some(),
            "Should find another_public_method"
        );

        use crate::symbol::Visibility as SymVis;

        assert_eq!(public_method.unwrap().visibility, SymVis::Public);
        assert_eq!(private_method.unwrap().visibility, SymVis::Private);
        assert_eq!(protected_method.unwrap().visibility, SymVis::Module);
        assert_eq!(another_public.unwrap().visibility, SymVis::Public);
    }

    #[test]
    fn test_attr_accessor() {
        let code = r#"
class User
  attr_accessor :name, :email
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find getter and setter methods
        let name_getter = symbols.iter().find(|s| {
            s.name.as_ref() == "name"
                && s.signature.as_ref().map(|s| s.as_ref()) == Some("def name")
        });
        let name_setter = symbols.iter().find(|s| {
            s.name.as_ref() == "name="
                && s.signature.as_ref().map(|s| s.as_ref()) == Some("def name=(value)")
        });
        let email_getter = symbols.iter().find(|s| {
            s.name.as_ref() == "email"
                && s.signature.as_ref().map(|s| s.as_ref()) == Some("def email")
        });
        let email_setter = symbols.iter().find(|s| {
            s.name.as_ref() == "email="
                && s.signature.as_ref().map(|s| s.as_ref()) == Some("def email=(value)")
        });

        assert!(name_getter.is_some(), "Should generate name getter");
        assert!(name_setter.is_some(), "Should generate name setter");
        assert!(email_getter.is_some(), "Should generate email getter");
        assert!(email_setter.is_some(), "Should generate email setter");
    }

    #[test]
    fn test_attr_reader() {
        let code = r#"
class User
  attr_reader :id, :username
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find getter methods only
        let id_getter = symbols
            .iter()
            .find(|s| s.name.as_ref() == "id" && s.kind == SymbolKind::Method);
        let username_getter = symbols
            .iter()
            .find(|s| s.name.as_ref() == "username" && s.kind == SymbolKind::Method);

        // Should NOT find setter methods
        let id_setter = symbols.iter().find(|s| s.name.as_ref() == "id=");
        let username_setter = symbols.iter().find(|s| s.name.as_ref() == "username=");

        assert!(id_getter.is_some(), "Should generate id getter");
        assert!(username_getter.is_some(), "Should generate username getter");
        assert!(id_setter.is_none(), "Should NOT generate id setter");
        assert!(
            username_setter.is_none(),
            "Should NOT generate username setter"
        );
    }

    #[test]
    fn test_attr_writer() {
        let code = r#"
class User
  attr_writer :password
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find setter method only
        let password_setter = symbols
            .iter()
            .find(|s| s.name.as_ref() == "password=" && s.kind == SymbolKind::Method);

        // Should NOT find getter method
        let password_getter = symbols
            .iter()
            .find(|s| s.name.as_ref() == "password" && s.kind == SymbolKind::Method);

        assert!(password_setter.is_some(), "Should generate password setter");
        assert!(
            password_getter.is_none(),
            "Should NOT generate password getter"
        );
    }

    #[test]
    fn test_method_parameters() {
        let code = r#"
class EdgeCases
  def simple(arg1, arg2)
  end

  def with_defaults(arg1, arg2 = "default")
  end

  def with_keywords(required:, optional: "default")
  end

  def with_splat(*args)
  end

  def with_kwargs(**kwargs)
  end

  def with_block(&block)
  end
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find methods with different parameter types
        let simple = symbols.iter().find(|s| s.name.as_ref() == "simple");
        let with_defaults = symbols.iter().find(|s| s.name.as_ref() == "with_defaults");
        let with_keywords = symbols.iter().find(|s| s.name.as_ref() == "with_keywords");
        let with_splat = symbols.iter().find(|s| s.name.as_ref() == "with_splat");
        let with_kwargs = symbols.iter().find(|s| s.name.as_ref() == "with_kwargs");
        let with_block = symbols.iter().find(|s| s.name.as_ref() == "with_block");

        assert!(simple.is_some(), "Should find simple method");
        assert_eq!(
            simple.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("def simple(arg1, arg2)")
        );

        assert!(with_defaults.is_some(), "Should find with_defaults method");
        assert_eq!(
            with_defaults
                .unwrap()
                .signature
                .as_ref()
                .map(|s| s.as_ref()),
            Some("def with_defaults(arg1, arg2 = \"default\")")
        );

        assert!(with_keywords.is_some(), "Should find with_keywords method");
        assert_eq!(
            with_keywords
                .unwrap()
                .signature
                .as_ref()
                .map(|s| s.as_ref()),
            Some("def with_keywords(required:, optional: \"default\")")
        );

        assert!(with_splat.is_some(), "Should find with_splat method");
        assert_eq!(
            with_splat.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("def with_splat(*args)")
        );

        assert!(with_kwargs.is_some(), "Should find with_kwargs method");
        assert_eq!(
            with_kwargs.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("def with_kwargs(**kwargs)")
        );

        assert!(with_block.is_some(), "Should find with_block method");
        assert_eq!(
            with_block.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("def with_block(&block)")
        );
    }

    // Phase 4 tests - Constant extraction

    #[test]
    fn test_constant_extraction_module() {
        let code = r#"
module Authentication
  VERSION = "1.0.0"
  DEFAULT_TIMEOUT = 30
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find constants
        let version = symbols.iter().find(|s| s.name.as_ref() == "VERSION");
        let timeout = symbols
            .iter()
            .find(|s| s.name.as_ref() == "DEFAULT_TIMEOUT");

        assert!(version.is_some(), "Should find VERSION constant");
        assert!(timeout.is_some(), "Should find DEFAULT_TIMEOUT constant");

        let version = version.unwrap();
        assert_eq!(version.kind, SymbolKind::Constant);
        assert_eq!(version.visibility, SymbolVisibility::Public);
        assert_eq!(
            version.signature.as_ref().map(|s| s.as_ref()),
            Some("VERSION = \"1.0.0\"")
        );

        let timeout = timeout.unwrap();
        assert_eq!(timeout.kind, SymbolKind::Constant);
        assert_eq!(
            timeout.signature.as_ref().map(|s| s.as_ref()),
            Some("DEFAULT_TIMEOUT = 30")
        );
    }

    #[test]
    fn test_constant_extraction_class() {
        let code = r#"
class User
  MAX_LOGIN_ATTEMPTS = 3
  DEFAULT_ROLE = "guest"
  PERMISSIONS = ["read", "write"]
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find constants
        let max_attempts = symbols
            .iter()
            .find(|s| s.name.as_ref() == "MAX_LOGIN_ATTEMPTS");
        let default_role = symbols.iter().find(|s| s.name.as_ref() == "DEFAULT_ROLE");
        let permissions = symbols.iter().find(|s| s.name.as_ref() == "PERMISSIONS");

        assert!(
            max_attempts.is_some(),
            "Should find MAX_LOGIN_ATTEMPTS constant"
        );
        assert!(default_role.is_some(), "Should find DEFAULT_ROLE constant");
        assert!(permissions.is_some(), "Should find PERMISSIONS constant");

        assert_eq!(
            max_attempts.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("MAX_LOGIN_ATTEMPTS = 3")
        );
        assert_eq!(
            default_role.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("DEFAULT_ROLE = \"guest\"")
        );
        assert_eq!(
            permissions.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("PERMISSIONS = [...]")
        );
    }

    #[test]
    fn test_constant_vs_variable() {
        let code = r#"
class User
  MAX_ATTEMPTS = 3
  @@user_count = 0

  def initialize(name)
    @name = name
    @age = 0
  end

  def process
    local_var = 123
    $global = "test"
  end
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Should find constant
        let max_attempts = symbols.iter().find(|s| s.name.as_ref() == "MAX_ATTEMPTS");
        assert!(max_attempts.is_some(), "Should find MAX_ATTEMPTS constant");
        assert_eq!(max_attempts.unwrap().kind, SymbolKind::Constant);

        // Should NOT find variables (per Phase 4 guidance: variables are not primary symbols)
        let class_var = symbols.iter().find(|s| s.name.as_ref() == "@@user_count");
        let instance_var = symbols.iter().find(|s| s.name.as_ref() == "@name");
        let local_var = symbols.iter().find(|s| s.name.as_ref() == "local_var");
        let global_var = symbols.iter().find(|s| s.name.as_ref() == "$global");

        assert!(class_var.is_none(), "Should NOT extract class variables");
        assert!(
            instance_var.is_none(),
            "Should NOT extract instance variables"
        );
        assert!(local_var.is_none(), "Should NOT extract local variables");
        assert!(global_var.is_none(), "Should NOT extract global variables");
    }

    #[test]
    fn test_constant_with_complex_value() {
        let code = r#"
class Config
  SIMPLE_STRING = "hello"
  SIMPLE_INT = 42
  SIMPLE_FLOAT = 3.14
  SIMPLE_BOOL = true
  SIMPLE_NIL = nil
  ARRAY_VALUE = [1, 2, 3]
  HASH_VALUE = { key: "value" }
  EXPRESSION = calculate_something()
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Check different value types in signatures
        let simple_string = symbols.iter().find(|s| s.name.as_ref() == "SIMPLE_STRING");
        let simple_int = symbols.iter().find(|s| s.name.as_ref() == "SIMPLE_INT");
        let simple_float = symbols.iter().find(|s| s.name.as_ref() == "SIMPLE_FLOAT");
        let simple_bool = symbols.iter().find(|s| s.name.as_ref() == "SIMPLE_BOOL");
        let simple_nil = symbols.iter().find(|s| s.name.as_ref() == "SIMPLE_NIL");
        let array_value = symbols.iter().find(|s| s.name.as_ref() == "ARRAY_VALUE");
        let hash_value = symbols.iter().find(|s| s.name.as_ref() == "HASH_VALUE");
        let expression = symbols.iter().find(|s| s.name.as_ref() == "EXPRESSION");

        assert_eq!(
            simple_string
                .unwrap()
                .signature
                .as_ref()
                .map(|s| s.as_ref()),
            Some("SIMPLE_STRING = \"hello\"")
        );
        assert_eq!(
            simple_int.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("SIMPLE_INT = 42")
        );
        assert_eq!(
            simple_float.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("SIMPLE_FLOAT = 3.14")
        );
        assert_eq!(
            simple_bool.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("SIMPLE_BOOL = true")
        );
        assert_eq!(
            simple_nil.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("SIMPLE_NIL = nil")
        );
        assert_eq!(
            array_value.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("ARRAY_VALUE = [...]")
        );
        assert_eq!(
            hash_value.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("HASH_VALUE = {{...}}")
        );
        assert_eq!(
            expression.unwrap().signature.as_ref().map(|s| s.as_ref()),
            Some("EXPRESSION = <expression>")
        );
    }

    #[test]
    fn test_constant_in_nested_scope() {
        let code = r#"
module Outer
  OUTER_CONST = "outer"

  module Inner
    INNER_CONST = "inner"
  end

  class MyClass
    CLASS_CONST = "class"
  end
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find constants at different nesting levels
        let outer_const = symbols.iter().find(|s| s.name.as_ref() == "OUTER_CONST");
        let inner_const = symbols.iter().find(|s| s.name.as_ref() == "INNER_CONST");
        let class_const = symbols.iter().find(|s| s.name.as_ref() == "CLASS_CONST");

        assert!(outer_const.is_some(), "Should find OUTER_CONST");
        assert!(inner_const.is_some(), "Should find INNER_CONST");
        assert!(class_const.is_some(), "Should find CLASS_CONST");

        assert_eq!(outer_const.unwrap().kind, SymbolKind::Constant);
        assert_eq!(inner_const.unwrap().kind, SymbolKind::Constant);
        assert_eq!(class_const.unwrap().kind, SymbolKind::Constant);
    }

    #[test]
    fn test_doc_comment_extraction() {
        let code = r#"
# This is a User class
# It manages user data
class User
  def initialize(name)
    @name = name
  end
end

# Calculate the sum of two numbers
# @param a [Integer] first number
# @param b [Integer] second number
# @return [Integer] the sum
def add(a, b)
  a + b
end

=begin
This is a multi-line comment
describing the Configuration module
It handles app configuration
=end
module Configuration
  VERSION = "1.0.0"
end

# Simple single-line comment
def simple_method
  puts "hello"
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).expect("Failed to create FileId");

        let symbols = parser.parse(code, file_id, &mut counter);

        // Find symbols with doc comments
        let user_class = symbols.iter().find(|s| s.name.as_ref() == "User");
        let add_method = symbols.iter().find(|s| s.name.as_ref() == "add");
        let config_module = symbols.iter().find(|s| s.name.as_ref() == "Configuration");
        let simple_method = symbols.iter().find(|s| s.name.as_ref() == "simple_method");

        // Test User class comment (multi-line single-line comments)
        assert!(user_class.is_some(), "Should find User class");
        let user_doc = user_class.unwrap().doc_comment.as_ref().map(|s| s.as_ref());
        assert!(user_doc.is_some(), "User class should have doc comment");
        assert!(
            user_doc.unwrap().contains("User class"),
            "Should contain 'User class'"
        );
        assert!(
            user_doc.unwrap().contains("manages user data"),
            "Should contain 'manages user data'"
        );

        // Test add method comment (YARD tags)
        assert!(add_method.is_some(), "Should find add method");
        let add_doc = add_method.unwrap().doc_comment.as_ref().map(|s| s.as_ref());
        assert!(add_doc.is_some(), "add method should have doc comment");
        assert!(
            add_doc.unwrap().contains("Calculate the sum"),
            "Should contain 'Calculate the sum'"
        );
        assert!(
            add_doc.unwrap().contains("@param a"),
            "Should preserve YARD @param tag"
        );
        assert!(
            add_doc.unwrap().contains("@return"),
            "Should preserve YARD @return tag"
        );

        // Test Configuration module comment (=begin...=end)
        assert!(config_module.is_some(), "Should find Configuration module");
        let config_doc = config_module
            .unwrap()
            .doc_comment
            .as_ref()
            .map(|s| s.as_ref());
        assert!(
            config_doc.is_some(),
            "Configuration module should have doc comment"
        );
        assert!(
            config_doc.unwrap().contains("multi-line comment"),
            "Should contain 'multi-line comment'"
        );
        assert!(
            config_doc.unwrap().contains("Configuration module"),
            "Should contain 'Configuration module'"
        );

        // Test simple_method comment (single-line)
        assert!(simple_method.is_some(), "Should find simple_method");
        let simple_doc = simple_method
            .unwrap()
            .doc_comment
            .as_ref()
            .map(|s| s.as_ref());
        assert!(
            simple_doc.is_some(),
            "simple_method should have doc comment"
        );
        assert_eq!(
            simple_doc.unwrap(),
            "Simple single-line comment",
            "Should match exact comment text"
        );
    }

    #[test]
    fn test_find_uses() {
        let code = r#"
class User
  def initialize(name)
    @name = name
  end
end

def test_method
  user = User.new("Alice")
  result = DataProcessor.process([1, 2, 3])
  admin = Admin.find(1)
  config = Configuration.instance
end

# Top-level usage
article = Article.create(title: "Test")
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let uses = parser.find_uses(code);

        println!("\n=== Found {} constant uses ===", uses.len());
        for (caller, constant, range) in &uses {
            println!(
                "  {} uses {} at {}:{}",
                caller, constant, range.start_line, range.start_column
            );
        }

        // Verify we found the constant uses
        assert!(uses.len() > 0, "Should find at least one constant use");

        // Check for specific patterns
        let user_uses: Vec<_> = uses
            .iter()
            .filter(|(_, constant, _)| *constant == "User")
            .collect();
        assert!(user_uses.len() > 0, "Should find User constant usage");

        let dataprocessor_uses: Vec<_> = uses
            .iter()
            .filter(|(_, constant, _)| *constant == "DataProcessor")
            .collect();
        assert!(
            dataprocessor_uses.len() > 0,
            "Should find DataProcessor constant usage"
        );

        let admin_uses: Vec<_> = uses
            .iter()
            .filter(|(_, constant, _)| *constant == "Admin")
            .collect();
        assert!(admin_uses.len() > 0, "Should find Admin constant usage");

        // Verify caller context
        let test_method_uses: Vec<_> = uses
            .iter()
            .filter(|(caller, _, _)| *caller == "test_method")
            .collect();
        assert!(
            test_method_uses.len() >= 3,
            "test_method should use at least 3 constants"
        );

        let module_uses: Vec<_> = uses
            .iter()
            .filter(|(caller, _, _)| *caller == "<module>")
            .collect();
        assert!(
            module_uses.len() > 0,
            "Should find top-level constant usage"
        );
    }

    #[test]
    fn test_find_uses_with_scope_resolution() {
        let code = r#"
module MyApp
  class User
  end
end

def test_scope
  user = MyApp::User.new
  parser = JSON::Parser.new
  result = ActiveRecord::Base.connection
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let uses = parser.find_uses(code);

        println!(
            "\n=== Scope Resolution Test - Found {} constant uses ===",
            uses.len()
        );
        for (caller, constant, range) in &uses {
            println!(
                "  {} uses {} at {}:{}",
                caller, constant, range.start_line, range.start_column
            );
        }

        // Verify we found scope resolution patterns
        assert!(
            uses.len() > 0,
            "Should find constant uses with scope resolution"
        );

        // Check for MyApp module reference
        let myapp_uses: Vec<_> = uses
            .iter()
            .filter(|(_, constant, _)| *constant == "MyApp")
            .collect();
        assert!(
            myapp_uses.len() > 0,
            "Should find MyApp in MyApp::User pattern"
        );

        // Check for JSON module reference
        let json_uses: Vec<_> = uses
            .iter()
            .filter(|(_, constant, _)| *constant == "JSON")
            .collect();
        assert!(
            json_uses.len() > 0,
            "Should find JSON in JSON::Parser pattern"
        );

        // Check for ActiveRecord module reference
        let ar_uses: Vec<_> = uses
            .iter()
            .filter(|(_, constant, _)| *constant == "ActiveRecord")
            .collect();
        assert!(
            ar_uses.len() > 0,
            "Should find ActiveRecord in ActiveRecord::Base pattern"
        );
    }

    #[test]
    fn test_find_uses_chained_methods() {
        let code = r#"
def test_chaining
  # Chained method calls - should extract receiver only once
  user = User.find(1).update(name: "test")
  result = DataProcessor.process(data).transform.save
  admin = Admin.where(active: true).first.reload
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let uses = parser.find_uses(code);

        println!(
            "\n=== Chained Methods Test - Found {} constant uses ===",
            uses.len()
        );
        for (caller, constant, range) in &uses {
            println!(
                "  {} uses {} at {}:{}",
                caller, constant, range.start_line, range.start_column
            );
        }

        // Should find constant receivers even in chained calls
        let user_uses: Vec<_> = uses
            .iter()
            .filter(|(_, constant, _)| *constant == "User")
            .collect();
        assert!(user_uses.len() > 0, "Should find User in chained call");

        let dp_uses: Vec<_> = uses
            .iter()
            .filter(|(_, constant, _)| *constant == "DataProcessor")
            .collect();
        assert!(
            dp_uses.len() > 0,
            "Should find DataProcessor in chained call"
        );
    }

    #[test]
    fn test_find_uses_nested_calls() {
        let code = r#"
def test_nested
  # Nested constant usage - should find all constants
  user = User.find(Admin.first.id)
  result = Processor.run(Config.load(Settings.default))
  data = Cache.fetch(Database.connection.query("SELECT * FROM users"))
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let uses = parser.find_uses(code);

        println!(
            "\n=== Nested Calls Test - Found {} constant uses ===",
            uses.len()
        );
        for (caller, constant, range) in &uses {
            println!(
                "  {} uses {} at {}:{}",
                caller, constant, range.start_line, range.start_column
            );
        }

        // Should find all nested constants
        let constants_found: Vec<&str> = uses.iter().map(|(_, constant, _)| *constant).collect();

        assert!(constants_found.contains(&"User"), "Should find User");
        assert!(constants_found.contains(&"Admin"), "Should find Admin");
        assert!(
            constants_found.contains(&"Processor"),
            "Should find Processor"
        );
        assert!(constants_found.contains(&"Config"), "Should find Config");
        assert!(
            constants_found.contains(&"Settings"),
            "Should find Settings"
        );
        assert!(constants_found.contains(&"Cache"), "Should find Cache");
        assert!(
            constants_found.contains(&"Database"),
            "Should find Database"
        );
    }

    #[test]
    fn test_find_uses_nil_receivers() {
        let code = r#"
def test_nil_receivers
  # Method calls without receivers or with lowercase receivers
  result = process(data)
  value = calculate()
  item = self.fetch
  data = @processor.run
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let uses = parser.find_uses(code);

        println!(
            "\n=== Nil Receivers Test - Found {} constant uses ===",
            uses.len()
        );
        for (caller, constant, range) in &uses {
            println!(
                "  {} uses {} at {}:{}",
                caller, constant, range.start_line, range.start_column
            );
        }

        // Should find NO constant uses (all receivers are nil, self, or lowercase)
        assert_eq!(
            uses.len(),
            0,
            "Should not find any constant uses with nil/lowercase receivers"
        );
    }

    #[test]
    fn test_find_uses_multi_level_scope() {
        let code = r#"
def test_multi_level
  # Multi-level scope resolution
  user = App::Models::User.create
  parser = Data::Processing::JSON::Parser.new
  config = System::Config::Database::Settings.load
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let uses = parser.find_uses(code);

        println!(
            "\n=== Multi-Level Scope Test - Found {} constant uses ===",
            uses.len()
        );
        for (caller, constant, range) in &uses {
            println!(
                "  {} uses {} at {}:{}",
                caller, constant, range.start_line, range.start_column
            );
        }

        // Should find module/constant references at each level
        let constants_found: Vec<&str> = uses.iter().map(|(_, constant, _)| *constant).collect();

        assert!(constants_found.contains(&"App"), "Should find App");
        assert!(constants_found.contains(&"Data"), "Should find Data");
        assert!(constants_found.contains(&"System"), "Should find System");
    }

    #[test]
    fn test_find_uses_mixed_case() {
        let code = r#"
def test_mixed_case
  # Only uppercase-starting identifiers are constants
  constant = ConstantName.method
  variable = variableName.method
  snake = snake_case.method
  camel = camelCase.method
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let uses = parser.find_uses(code);

        println!(
            "\n=== Mixed Case Test - Found {} constant uses ===",
            uses.len()
        );
        for (caller, constant, range) in &uses {
            println!(
                "  {} uses {} at {}:{}",
                caller, constant, range.start_line, range.start_column
            );
        }

        // Should only find ConstantName (uppercase start)
        assert_eq!(uses.len(), 1, "Should find exactly one constant use");
        let (_, constant, _) = uses[0];
        assert_eq!(constant, "ConstantName", "Should only find ConstantName");
    }

    #[test]
    fn test_find_uses_function_context() {
        let code = r#"
# Top-level
TopLevel.call

class MyClass
  # Class body
  ClassLevel.call

  def instance_method
    InstanceLevel.call
  end

  def self.class_method
    ClassMethodLevel.call
  end
end

def standalone_function
  StandaloneLevel.call
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let uses = parser.find_uses(code);

        println!(
            "\n=== Function Context Test - Found {} constant uses ===",
            uses.len()
        );
        for (caller, constant, range) in &uses {
            println!(
                "  {} uses {} at {}:{}",
                caller, constant, range.start_line, range.start_column
            );
        }

        // Verify correct caller context tracking
        let contexts: Vec<(&str, &str)> = uses
            .iter()
            .map(|(caller, constant, _)| (*caller, *constant))
            .collect();

        assert!(
            contexts.contains(&("<module>", "TopLevel")),
            "Should track top-level context"
        );
        assert!(
            contexts.contains(&("instance_method", "InstanceLevel")),
            "Should track instance method context"
        );
        assert!(
            contexts.contains(&("class_method", "ClassMethodLevel")),
            "Should track class method context"
        );
        assert!(
            contexts.contains(&("standalone_function", "StandaloneLevel")),
            "Should track standalone function context"
        );
    }

    #[test]
    fn test_find_uses_real_world_url_formatter() {
        // Real-world validation with UrlFormatter.rb from guliveo
        // Note: Using r##"..."## to avoid conflicts with Ruby's #{...} syntax
        let code = r##"
module UrlFormatter
  class << self
    def encode(url)
      stripped_url = url.strip
      URI.parse(stripped_url).to_s
    rescue URI::InvalidURIError
      Addressable::URI.escape(stripped_url).to_s
    end

    def display_url(url)
      return nil if url.blank?
      output = CGI.unescape url
      output.presence || ''
    rescue ArgumentError
      url = CGI.escape(url)
    end

    def reduce(url)
      keyword_reducer = KeywordReducer.new
      keyword_reducer.perform(url.parameterize, separator: '-')
    end

    def root_url(url)
      Seo::Cms.subdomain?(url) ? hostname(url) : root_domain(url)
    end

    def normalized_md5(url)
      Digest::MD5.hexdigest(normalize_for_md5(url))
    end

    def postrank_parse(url)
      stripped_url = url.strip
      return nil if invalid?(stripped_url)
      PostRank::URI.parse(stripped_url)
    rescue Addressable::URI::InvalidURIError
      OpenStruct.new(scheme: nil, host: nil)
    end

    def build_host_domain(subdomain:)
      domain = SETTINGS[:demo_domain]
      "#{subdomain}.#{domain}"
    end

    def build_host_subdomain(email)
      return SecureRandom.hex(10) if email.blank?
      avoid_phishing_like_subdomains(email.parameterize)
    end
  end
end
"##;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let uses = parser.find_uses(code);

        println!(
            "\n=== Real-World UrlFormatter Test - Found {} constant uses ===",
            uses.len()
        );
        for (caller, constant, range) in &uses {
            println!(
                "  {} uses {} at {}:{}",
                caller, constant, range.start_line, range.start_column
            );
        }

        // Verify we extract expected constants from real Ruby code
        let constants_found: Vec<&str> = uses.iter().map(|(_, constant, _)| *constant).collect();

        // Core Ruby/external library constants used in method call patterns
        assert!(constants_found.contains(&"URI"), "Should find URI constant");
        assert!(
            constants_found.contains(&"Addressable"),
            "Should find Addressable module"
        );
        assert!(constants_found.contains(&"CGI"), "Should find CGI constant");
        assert!(
            constants_found.contains(&"KeywordReducer"),
            "Should find KeywordReducer class"
        );
        assert!(constants_found.contains(&"Seo"), "Should find Seo module");
        assert!(
            constants_found.contains(&"Digest"),
            "Should find Digest module"
        );
        assert!(
            constants_found.contains(&"PostRank"),
            "Should find PostRank module"
        );
        assert!(
            constants_found.contains(&"OpenStruct"),
            "Should find OpenStruct class"
        );
        assert!(
            constants_found.contains(&"SecureRandom"),
            "Should find SecureRandom module"
        );

        // Note: SETTINGS[:demo_domain] uses array/hash access, not method call pattern
        // Current implementation focuses on ConstantName.method_call patterns

        // Verify caller context for specific methods
        let encode_uses: Vec<_> = uses
            .iter()
            .filter(|(caller, _, _)| *caller == "encode")
            .collect();
        assert!(
            encode_uses.len() >= 2,
            "encode method should use multiple constants (URI, Addressable)"
        );

        let display_url_uses: Vec<_> = uses
            .iter()
            .filter(|(caller, _, _)| *caller == "display_url")
            .collect();
        assert!(
            display_url_uses.len() >= 2,
            "display_url method should use CGI constant"
        );

        // Ensure we're tracking relationships correctly - should find substantial uses
        assert!(
            uses.len() >= 15,
            "Should find at least 15 constant uses in UrlFormatter (found {})",
            uses.len()
        );
    }

    #[test]
    fn test_find_implementations_basic_mixins() {
        let code = r#"
module Loggable
  def log(msg)
    puts msg
  end
end

module Cacheable
end

class User
  include Loggable
  extend Cacheable
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let implementations = parser.find_implementations(code);

        println!(
            "\n=== Basic Mixin Test - Found {} implementations ===",
            implementations.len()
        );
        for (implementer, module_name, range) in &implementations {
            println!(
                "  {} mixes in {} at line {}",
                implementer,
                module_name,
                range.start_line + 1
            );
        }

        assert_eq!(
            implementations.len(),
            2,
            "Should find 2 mixin implementations"
        );

        // Verify User includes Loggable
        let user_loggable = implementations
            .iter()
            .find(|(impl_name, mod_name, _)| *impl_name == "User" && *mod_name == "Loggable");
        assert!(user_loggable.is_some(), "User should include Loggable");

        // Verify User extends Cacheable
        let user_cacheable = implementations
            .iter()
            .find(|(impl_name, mod_name, _)| *impl_name == "User" && *mod_name == "Cacheable");
        assert!(user_cacheable.is_some(), "User should extend Cacheable");
    }

    #[test]
    fn test_find_implementations_qualified_names() {
        let code = r#"
module Features::Security
end

class Admin
  include Features::Security
  prepend AuditLog
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let implementations = parser.find_implementations(code);

        println!(
            "\n=== Qualified Names Test - Found {} implementations ===",
            implementations.len()
        );
        for (implementer, module_name, range) in &implementations {
            println!(
                "  {} mixes in {} at line {}",
                implementer,
                module_name,
                range.start_line + 1
            );
        }

        assert_eq!(
            implementations.len(),
            2,
            "Should find 2 mixin implementations"
        );

        // Verify qualified name is extracted correctly
        let admin_security = implementations.iter().find(|(impl_name, mod_name, _)| {
            *impl_name == "Admin" && *mod_name == "Features::Security"
        });
        assert!(
            admin_security.is_some(),
            "Admin should include Features::Security with qualified name"
        );

        // Verify prepend
        let admin_audit = implementations
            .iter()
            .find(|(impl_name, mod_name, _)| *impl_name == "Admin" && *mod_name == "AuditLog");
        assert!(admin_audit.is_some(), "Admin should prepend AuditLog");
    }

    #[test]
    fn test_find_implementations_multiple_mixins() {
        let code = r#"
class Service
  include A, B, C
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let implementations = parser.find_implementations(code);

        println!(
            "\n=== Multiple Mixins Test - Found {} implementations ===",
            implementations.len()
        );
        for (implementer, module_name, range) in &implementations {
            println!(
                "  {} mixes in {} at line {}",
                implementer,
                module_name,
                range.start_line + 1
            );
        }

        assert_eq!(
            implementations.len(),
            3,
            "Should find 3 mixin implementations from 'include A, B, C'"
        );

        // Verify all three modules are extracted
        let modules: Vec<&str> = implementations
            .iter()
            .map(|(_, mod_name, _)| *mod_name)
            .collect();
        assert!(modules.contains(&"A"), "Should include A");
        assert!(modules.contains(&"B"), "Should include B");
        assert!(modules.contains(&"C"), "Should include C");
    }

    #[test]
    fn test_find_implementations_nested_classes() {
        let code = r#"
module Application
  class ServiceClass
    include Loggable

    class InnerClass
      extend Cacheable
    end
  end
end
"#;

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let implementations = parser.find_implementations(code);

        println!(
            "\n=== Nested Classes Test - Found {} implementations ===",
            implementations.len()
        );
        for (implementer, module_name, range) in &implementations {
            println!(
                "  {} mixes in {} at line {}",
                implementer,
                module_name,
                range.start_line + 1
            );
        }

        assert_eq!(
            implementations.len(),
            2,
            "Should find 2 mixin implementations in nested classes"
        );

        // Verify ServiceClass includes Loggable
        let service_loggable = implementations.iter().find(|(impl_name, mod_name, _)| {
            *impl_name == "ServiceClass" && *mod_name == "Loggable"
        });
        assert!(
            service_loggable.is_some(),
            "ServiceClass should include Loggable"
        );

        // Verify InnerClass extends Cacheable
        let inner_cacheable = implementations.iter().find(|(impl_name, mod_name, _)| {
            *impl_name == "InnerClass" && *mod_name == "Cacheable"
        });
        assert!(
            inner_cacheable.is_some(),
            "InnerClass should extend Cacheable"
        );
    }

    #[test]
    fn test_find_implementations_comprehensive() {
        // Test with the comprehensive.rb test file
        let code = include_str!("../../../examples/ruby/comprehensive.rb");

        let mut parser = RubyParser::new().expect("Failed to create parser");
        let implementations = parser.find_implementations(code);

        println!(
            "\n=== Comprehensive Ruby Test - Found {} implementations ===",
            implementations.len()
        );

        // Show sample of implementations
        for (implementer, module_name, range) in implementations.iter().take(10) {
            println!(
                "  {} mixes in {} at line {}",
                implementer,
                module_name,
                range.start_line + 1
            );
        }
        if implementations.len() > 10 {
            println!("  ... and {} more", implementations.len() - 10);
        }

        // The comprehensive.rb file has 27 mixin statements (verified with grep)
        // This includes: include, extend, and prepend with various patterns
        assert!(
            implementations.len() >= 27,
            "Should find at least 27 mixin implementations in comprehensive.rb (found {})",
            implementations.len()
        );

        // Verify some specific examples from comprehensive.rb

        // Verify common mixins from the file
        let has_loggable = implementations
            .iter()
            .any(|(_, mod_name, _)| *mod_name == "Loggable");
        assert!(has_loggable, "Should find Loggable mixin");

        // Verify qualified module names (e.g., Features::Security, Authentication::OAuth)
        let has_qualified = implementations
            .iter()
            .any(|(_, mod_name, _)| mod_name.contains("::"));
        assert!(
            has_qualified,
            "Should find qualified module names (e.g., Features::Security)"
        );

        // Verify prepend is detected
        let has_prepend = implementations
            .iter()
            .any(|(_, mod_name, _)| *mod_name == "Auditable" || *mod_name == "TrackingA");
        assert!(has_prepend, "Should find prepend mixins");

        // Verify multiple mixins in single statement are all captured
        // Look for cases where multiple modules are included at once
        let implementers: std::collections::HashSet<&str> = implementations
            .iter()
            .map(|(impl_name, _, _)| *impl_name)
            .collect();
        assert!(
            implementers.len() >= 10,
            "Should find mixins in at least 10 different classes"
        );
    }
}
