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
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Language, LanguageParser, MethodCall, NodeTracker, NodeTrackingState,
    ParserContext, ScopeType,
};
use crate::symbol::Visibility as SymbolVisibility;
use crate::parsing::context::Visibility as ContextVisibility;
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
                if let Some(class_symbol) = self.process_class(node, code, file_id, counter, context) {
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
                if let Some(module_symbol) = self.process_module(node, code, file_id, counter, context) {
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
                if let Some(method_symbol) = self.process_method(node, code, file_id, counter, context) {
                    symbols.push(method_symbol);
                }

                // Process method body
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "singleton_method" => {
                self.register_handled_node(node.kind(), node.kind_id());

                // Extract singleton (class) method symbol
                if let Some(method_symbol) = self.process_singleton_method(node, code, file_id, counter, context) {
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
                if let Some(constant_symbol) = self.process_assignment(node, code, file_id, counter, context) {
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

        // TODO: Phase 4 - Extract documentation comment
        // Ruby uses: # single-line, =begin...=end multi-line, YARD tags

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

        // TODO: Phase 4 - Extract documentation comment

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

        // TODO: Phase 4 - Extract documentation comment

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

        // TODO: Phase 4 - Extract documentation comment

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
    fn extract_method_signature(&self, node: Node, code: &str, method_name: &str, is_singleton: bool) -> String {
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
                    self.generate_synthetic_methods(node, code, file_id, symbols, counter, context, method_text);
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
                    self.process_children_for_method_calls(node, code, method_calls, current_function);
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
        let receiver = node.child_by_field_name("receiver")
            .map(|r| &code[r.byte_range()]);

        let mut method_call = MethodCall::new(caller, method_name, range);
        if let Some(recv) = receiver {
            method_call = method_call.with_receiver(recv);
        }

        Some(method_call)
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
    fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        Self::parse(self, code, file_id, counter)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn language(&self) -> Language {
        Language::Ruby
    }

    fn extract_doc_comment(&self, _node: &Node, _code: &str) -> Option<String> {
        // TODO: Phase 2 - Ruby documentation comments
        // Ruby uses: # single-line comments, =begin...=end multi-line comments
        // YARD documentation: # @param, # @return, etc.
        None
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

    fn find_implementations<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // TODO: Phase 5 - Module inclusions (include/prepend)
        Vec::new()
    }

    fn find_uses<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // TODO: Phase 5 - Type usage tracking
        Vec::new()
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
        assert!(symbols.len() >= 2, "Should have at least module and User class");
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
        assert_eq!(user_class.signature.as_ref().map(|s| s.as_ref()), Some("class User"));
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
        assert_eq!(user_class.signature.as_ref().map(|s| s.as_ref()), Some("class User"));

        // Verify Admin class has User as superclass
        assert_eq!(admin_class.signature.as_ref().map(|s| s.as_ref()), Some("class Admin < User"));
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
        assert_eq!(auth_module.signature.as_ref().map(|s| s.as_ref()), Some("module Authentication"));
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
        assert_eq!(initialize.signature.as_ref().map(|s| s.as_ref()), Some("def initialize(name)"));

        let greet = greet.unwrap();
        assert_eq!(greet.kind, SymbolKind::Method);
        assert_eq!(greet.signature.as_ref().map(|s| s.as_ref()), Some("def greet"));
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
        assert_eq!(find.signature.as_ref().map(|s| s.as_ref()), Some("def self.find(id)"));

        let count = count.unwrap();
        assert_eq!(count.kind, SymbolKind::Method);
        assert_eq!(count.signature.as_ref().map(|s| s.as_ref()), Some("def self.count"));
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
        let protected_method = symbols.iter().find(|s| s.name.as_ref() == "protected_method");
        let another_public = symbols.iter().find(|s| s.name.as_ref() == "another_public_method");

        assert!(public_method.is_some(), "Should find public_method");
        assert!(private_method.is_some(), "Should find private_method");
        assert!(protected_method.is_some(), "Should find protected_method");
        assert!(another_public.is_some(), "Should find another_public_method");

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
        let name_getter = symbols.iter().find(|s| s.name.as_ref() == "name" && s.signature.as_ref().map(|s| s.as_ref()) == Some("def name"));
        let name_setter = symbols.iter().find(|s| s.name.as_ref() == "name=" && s.signature.as_ref().map(|s| s.as_ref()) == Some("def name=(value)"));
        let email_getter = symbols.iter().find(|s| s.name.as_ref() == "email" && s.signature.as_ref().map(|s| s.as_ref()) == Some("def email"));
        let email_setter = symbols.iter().find(|s| s.name.as_ref() == "email=" && s.signature.as_ref().map(|s| s.as_ref()) == Some("def email=(value)"));

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
        let id_getter = symbols.iter().find(|s| s.name.as_ref() == "id" && s.kind == SymbolKind::Method);
        let username_getter = symbols.iter().find(|s| s.name.as_ref() == "username" && s.kind == SymbolKind::Method);

        // Should NOT find setter methods
        let id_setter = symbols.iter().find(|s| s.name.as_ref() == "id=");
        let username_setter = symbols.iter().find(|s| s.name.as_ref() == "username=");

        assert!(id_getter.is_some(), "Should generate id getter");
        assert!(username_getter.is_some(), "Should generate username getter");
        assert!(id_setter.is_none(), "Should NOT generate id setter");
        assert!(username_setter.is_none(), "Should NOT generate username setter");
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
        let password_setter = symbols.iter().find(|s| s.name.as_ref() == "password=" && s.kind == SymbolKind::Method);

        // Should NOT find getter method
        let password_getter = symbols.iter().find(|s| s.name.as_ref() == "password" && s.kind == SymbolKind::Method);

        assert!(password_setter.is_some(), "Should generate password setter");
        assert!(password_getter.is_none(), "Should NOT generate password getter");
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
        assert_eq!(simple.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("def simple(arg1, arg2)"));

        assert!(with_defaults.is_some(), "Should find with_defaults method");
        assert_eq!(with_defaults.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("def with_defaults(arg1, arg2 = \"default\")"));

        assert!(with_keywords.is_some(), "Should find with_keywords method");
        assert_eq!(with_keywords.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("def with_keywords(required:, optional: \"default\")"));

        assert!(with_splat.is_some(), "Should find with_splat method");
        assert_eq!(with_splat.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("def with_splat(*args)"));

        assert!(with_kwargs.is_some(), "Should find with_kwargs method");
        assert_eq!(with_kwargs.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("def with_kwargs(**kwargs)"));

        assert!(with_block.is_some(), "Should find with_block method");
        assert_eq!(with_block.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("def with_block(&block)"));
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
        let timeout = symbols.iter().find(|s| s.name.as_ref() == "DEFAULT_TIMEOUT");

        assert!(version.is_some(), "Should find VERSION constant");
        assert!(timeout.is_some(), "Should find DEFAULT_TIMEOUT constant");

        let version = version.unwrap();
        assert_eq!(version.kind, SymbolKind::Constant);
        assert_eq!(version.visibility, SymbolVisibility::Public);
        assert_eq!(version.signature.as_ref().map(|s| s.as_ref()), Some("VERSION = \"1.0.0\""));

        let timeout = timeout.unwrap();
        assert_eq!(timeout.kind, SymbolKind::Constant);
        assert_eq!(timeout.signature.as_ref().map(|s| s.as_ref()), Some("DEFAULT_TIMEOUT = 30"));
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
        let max_attempts = symbols.iter().find(|s| s.name.as_ref() == "MAX_LOGIN_ATTEMPTS");
        let default_role = symbols.iter().find(|s| s.name.as_ref() == "DEFAULT_ROLE");
        let permissions = symbols.iter().find(|s| s.name.as_ref() == "PERMISSIONS");

        assert!(max_attempts.is_some(), "Should find MAX_LOGIN_ATTEMPTS constant");
        assert!(default_role.is_some(), "Should find DEFAULT_ROLE constant");
        assert!(permissions.is_some(), "Should find PERMISSIONS constant");

        assert_eq!(max_attempts.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("MAX_LOGIN_ATTEMPTS = 3"));
        assert_eq!(default_role.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("DEFAULT_ROLE = \"guest\""));
        assert_eq!(permissions.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("PERMISSIONS = [...]"));
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
        assert!(instance_var.is_none(), "Should NOT extract instance variables");
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

        assert_eq!(simple_string.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("SIMPLE_STRING = \"hello\""));
        assert_eq!(simple_int.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("SIMPLE_INT = 42"));
        assert_eq!(simple_float.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("SIMPLE_FLOAT = 3.14"));
        assert_eq!(simple_bool.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("SIMPLE_BOOL = true"));
        assert_eq!(simple_nil.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("SIMPLE_NIL = nil"));
        assert_eq!(array_value.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("ARRAY_VALUE = [...]"));
        assert_eq!(hash_value.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("HASH_VALUE = {{...}}"));
        assert_eq!(expression.unwrap().signature.as_ref().map(|s| s.as_ref()), Some("EXPRESSION = <expression>"));
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
}
