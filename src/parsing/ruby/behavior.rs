//! Ruby-specific language behavior implementation

use crate::parsing::LanguageBehavior;
use crate::parsing::ResolutionScope;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::storage::DocumentIndex;
use crate::{FileId, SymbolId, Visibility};
use std::path::{Path, PathBuf};
use tree_sitter::Language;

/// Ruby language behavior implementation
#[derive(Clone)]
pub struct RubyBehavior {
    language: Language,
    state: BehaviorState,
}

impl RubyBehavior {
    /// Create a new Ruby behavior instance
    pub fn new() -> Self {
        Self {
            language: tree_sitter_ruby::LANGUAGE.into(),
            state: BehaviorState::new(),
        }
    }

    /// Resolve Ruby relative imports (require_relative)
    fn resolve_ruby_relative_require(&self, require_path: &str, from_module: &str) -> String {
        // Ruby require_relative is relative to the current file's directory
        // Split the current module path
        let mut parts: Vec<_> = from_module.split("::").collect();

        // Remove the current file/module name (last part)
        if !parts.is_empty() {
            parts.pop();
        }

        // Add the require path
        if !require_path.is_empty() {
            // Split the require path and add each part
            for part in require_path.split('/') {
                if !part.is_empty() && part != "." {
                    if part == ".." {
                        // Go up one level
                        if !parts.is_empty() {
                            parts.pop();
                        }
                    } else {
                        parts.push(part);
                    }
                }
            }
        }

        parts.join("::")
    }
}

impl StatefulBehavior for RubyBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl Default for RubyBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for RubyBehavior {
    fn configure_symbol(&self, symbol: &mut crate::Symbol, module_path: Option<&str>) {
        // Apply default behavior: set module_path and parse visibility
        if let Some(path) = module_path {
            let full_path = self.format_module_path(path, &symbol.name);
            symbol.module_path = Some(full_path.clone().into());

            // If this is the synthetic module symbol, set its display name to the last segment
            // (e.g., Namespace::SubModule -> SubModule)
            if symbol.kind == crate::types::SymbolKind::Module {
                let short = full_path.rsplit("::").next().unwrap_or(full_path.as_str());
                symbol.name = crate::types::compact_string(short);
            }
        } else if symbol.kind == crate::types::SymbolKind::Module {
            // No module path available (e.g., root module)
            symbol.name = crate::types::compact_string("module");
        }

        if let Some(ref sig) = symbol.signature {
            symbol.visibility = self.parse_visibility(sig);
        }
    }

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(crate::parsing::resolution::GenericResolutionContext::new(
            file_id,
        ))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn crate::parsing::InheritanceResolver> {
        Box::new(crate::parsing::resolution::GenericInheritanceResolver::new())
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        // Ruby uses file paths as module paths, not including the symbol name
        // Similar to Python but with :: separator
        base_path.to_string()
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // Ruby uses explicit visibility keywords
        // Check for visibility modifiers
        if signature.contains("private ")
            || signature.starts_with("private ")
            || signature.contains("\nprivate\n")
            || signature.contains("private\n")
        {
            Visibility::Private
        } else if signature.contains("protected ")
            || signature.starts_with("protected ")
            || signature.contains("\nprotected\n")
            || signature.contains("protected\n")
        {
            Visibility::Module // Ruby's protected is like module-level
        } else if signature.contains("public ")
            || signature.starts_with("public ")
            || signature.contains("\npublic\n")
            || signature.contains("public\n")
        {
            Visibility::Public
        } else {
            // In Ruby, methods are public by default
            Visibility::Public
        }
    }

    fn module_separator(&self) -> &'static str {
        "::"
    }

    fn supports_traits(&self) -> bool {
        false // Ruby doesn't have traits, it uses modules and mixins
    }

    fn supports_inherent_methods(&self) -> bool {
        true // Ruby modules and classes have methods
    }

    fn get_language(&self) -> Language {
        self.language.clone()
    }

    fn normalize_caller_name(&self, name: &str, file_id: FileId) -> String {
        // Ruby doesn't have synthetic caller markers like Python's <module>
        // Top-level code is typically in a class or module context
        if name == "<main>" {
            if let Some(module_path) = self.get_module_path_for_file(file_id) {
                module_path
                    .rsplit("::")
                    .next()
                    .unwrap_or("main")
                    .to_string()
            } else {
                "main".to_string()
            }
        } else {
            name.to_string()
        }
    }

    fn resolve_external_call_target(
        &self,
        to_name: &str,
        from_file: FileId,
    ) -> Option<(String, String)> {
        // Use tracked imports to infer the module for an unresolved callee
        let imports = self.get_imports_for_file(from_file);
        // Prefer explicit imports that name the symbol
        for imp in &imports {
            // Aliased import: require 'module' as Alias
            if let Some(alias) = &imp.alias {
                if alias == to_name {
                    // Map to the base module path
                    if let Some((module_path, _real_name)) = imp.path.rsplit_once("::") {
                        return Some((module_path.to_string(), to_name.to_string()));
                    }
                }
            }
            // require 'module::Name'
            if imp.path.ends_with(&format!("::{to_name}")) {
                if let Some((module_path, _)) = imp.path.rsplit_once("::") {
                    return Some((module_path.to_string(), to_name.to_string()));
                }
            }
        }
        None
    }

    fn create_external_symbol(
        &self,
        document_index: &mut crate::storage::DocumentIndex,
        module_path: &str,
        symbol_name: &str,
        language_id: crate::parsing::LanguageId,
    ) -> crate::IndexResult<crate::SymbolId> {
        use crate::storage::MetadataKey;
        use crate::{IndexError, Symbol, SymbolId, SymbolKind, Visibility};

        // Reuse existing external symbol if present
        if let Ok(cands) = document_index.find_symbols_by_name(symbol_name, None) {
            for s in cands {
                if let Some(mp) = &s.module_path {
                    if mp.as_ref() == module_path {
                        return Ok(s.id);
                    }
                }
            }
        }

        // Compute virtual file path for Ruby stubs
        let mut path_buf = String::from(".codanna/external/");
        path_buf.push_str(&module_path.replace("::", "/"));
        path_buf.push_str(".rb");
        let path_str = path_buf;

        // Ensure file_info exists
        let file_id = if let Ok(Some((fid, _))) = document_index.get_file_info(&path_str) {
            fid
        } else {
            let next_file_id =
                document_index
                    .get_next_file_id()
                    .map_err(|e| IndexError::TantivyError {
                        operation: "get_next_file_id".to_string(),
                        cause: e.to_string(),
                    })?;
            let file_id = crate::FileId::new(next_file_id).ok_or(IndexError::FileIdExhausted)?;
            let hash = format!("external:{module_path}");
            let ts = crate::indexing::get_utc_timestamp();
            document_index
                .store_file_info(file_id, &path_str, &hash, ts)
                .map_err(|e| IndexError::TantivyError {
                    operation: "store_file_info".to_string(),
                    cause: e.to_string(),
                })?;
            file_id
        };

        // Allocate a new symbol id
        let next_id =
            document_index
                .get_next_symbol_id()
                .map_err(|e| IndexError::TantivyError {
                    operation: "get_next_symbol_id".to_string(),
                    cause: e.to_string(),
                })?;
        let symbol_id = SymbolId::new(next_id).ok_or(IndexError::SymbolIdExhausted)?;

        // Build and index the external symbol as a Class (Ruby classes)
        let mut symbol = Symbol::new(
            symbol_id,
            symbol_name.to_string(),
            SymbolKind::Class,
            file_id,
            crate::Range::new(0, 0, 0, 0),
        )
        .with_visibility(Visibility::Public);
        symbol.module_path = Some(module_path.to_string().into());
        symbol.scope_context = Some(crate::symbol::ScopeContext::Global);
        symbol.language_id = Some(language_id);

        document_index
            .index_symbol(&symbol, &path_str)
            .map_err(|e| IndexError::TantivyError {
                operation: "index_symbol".to_string(),
                cause: e.to_string(),
            })?;

        // Update symbol counter metadata
        document_index
            .store_metadata(MetadataKey::SymbolCounter, symbol_id.value() as u64)
            .map_err(|e| IndexError::TantivyError {
                operation: "store_metadata(SymbolCounter)".to_string(),
                cause: e.to_string(),
            })?;

        Ok(symbol_id)
    }

    fn module_path_from_file(&self, file_path: &Path, project_root: &Path) -> Option<String> {
        // Get relative path from project root
        let relative_path = file_path.strip_prefix(project_root).ok()?;

        // Convert path to string
        let path_str = relative_path.to_str()?;

        // Remove common Ruby source directories if present
        let path_without_src = path_str
            .strip_prefix("lib/")
            .or_else(|| path_str.strip_prefix("app/"))
            .or_else(|| path_str.strip_prefix("src/"))
            .unwrap_or(path_str);

        // Remove the .rb extension and other Ruby extensions
        let path_without_ext = path_without_src
            .strip_suffix(".rb")
            .or_else(|| path_without_src.strip_suffix(".rake"))
            .or_else(|| path_without_src.strip_suffix(".gemspec"))
            .unwrap_or(path_without_src);

        // Convert path separators to Ruby module separators
        let module_path = path_without_ext.replace('/', "::");

        // Handle special cases
        if module_path.is_empty() {
            None
        } else {
            Some(module_path)
        }
    }

    // Override import tracking methods to use state

    fn register_file(&self, path: PathBuf, file_id: FileId, module_path: String) {
        self.register_file_with_state(path, file_id, module_path);
    }

    fn add_import(&self, import: crate::parsing::Import) {
        self.add_import_with_state(import);
    }

    fn get_imports_for_file(&self, file_id: FileId) -> Vec<crate::parsing::Import> {
        self.get_imports_from_state(file_id)
    }

    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        // Use the BehaviorState to get module path (O(1) lookup)
        self.state.get_module_path(file_id)
    }

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        importing_module: Option<&str>,
    ) -> bool {
        // 1. Always check exact match first (performance)
        if import_path == symbol_module_path {
            if crate::config::is_global_debug_enabled() {
                eprintln!("DEBUG: Ruby exact match: {import_path} == {symbol_module_path}");
            }
            return true;
        }

        // 2. Handle Ruby-specific import patterns
        if let Some(importing_mod) = importing_module {
            if crate::config::is_global_debug_enabled() {
                eprintln!(
                    "DEBUG: Ruby import_matches_symbol: import='{import_path}', symbol='{symbol_module_path}', from='{importing_mod}'"
                );
            }
            // Handle relative requires (require_relative)
            if import_path.starts_with('.') || import_path.starts_with("./") {
                let resolved = self.resolve_ruby_relative_require(import_path, importing_mod);
                if resolved == symbol_module_path {
                    return true;
                }
            }

            // Handle absolute imports that might be partial
            // e.g., "Module::Func" might match "Namespace::Module::Func"
            if !import_path.contains("::") {
                // Simple module name, might be imported directly
                if symbol_module_path.ends_with(&format!("::{import_path}")) {
                    return true;
                }
            } else {
                // Multi-part import path
                // Check if it's a suffix of the symbol path
                if symbol_module_path.ends_with(import_path) {
                    return true;
                }
            }
        }

        false
    }

    fn resolve_import_path_with_context(
        &self,
        import_path: &str,
        importing_module: Option<&str>,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // Split the path using Ruby's module separator
        let separator = self.module_separator();
        let segments: Vec<&str> = import_path.split(separator).collect();

        if segments.is_empty() {
            return None;
        }

        // The symbol name is the last segment
        let symbol_name = segments.last()?;

        // Find symbols with this name (using index for performance)
        let candidates = document_index
            .find_symbols_by_name(symbol_name, None)
            .ok()?;

        // Find the one with matching module path using Ruby-specific rules
        for candidate in &candidates {
            if let Some(module_path) = &candidate.module_path {
                if self.import_matches_symbol(import_path, module_path.as_ref(), importing_module) {
                    return Some(candidate.id);
                }
            }
        }

        None
    }

    fn build_resolution_context(
        &self,
        file_id: FileId,
        document_index: &DocumentIndex,
    ) -> crate::error::IndexResult<Box<dyn crate::parsing::ResolutionScope>> {
        use crate::error::IndexError;
        use crate::parsing::resolution::GenericResolutionContext;

        // Create Ruby-specific resolution context
        let mut context = GenericResolutionContext::new(file_id);

        // 1. Add imported symbols
        let imports = self.get_imports_for_file(file_id);
        for import in imports {
            if let Some(symbol_id) = self.resolve_import(&import, document_index) {
                // Use alias if provided, otherwise use the name
                let name = if let Some(alias) = &import.alias {
                    alias.clone()
                } else {
                    // For "require 'module::name'", use just the name
                    // For "require 'module'", use the module name
                    if import.path.contains("::") {
                        import
                            .path
                            .rsplit("::")
                            .next()
                            .unwrap_or(&import.path)
                            .to_string()
                    } else {
                        import.path.clone()
                    }
                };

                // Add to imported symbols
                context.add_symbol(name, symbol_id, crate::parsing::ScopeLevel::Package);
            }
        }

        // 2. Add file's module-level symbols
        let file_symbols =
            document_index
                .find_symbols_by_file(file_id)
                .map_err(|e| IndexError::TantivyError {
                    operation: "find_symbols_by_file".to_string(),
                    cause: e.to_string(),
                })?;

        for symbol in file_symbols {
            if self.is_resolvable_symbol(&symbol) {
                // Determine the appropriate scope level
                let scope_level = match symbol.scope_context {
                    Some(crate::symbol::ScopeContext::Module) => crate::parsing::ScopeLevel::Module,
                    Some(crate::symbol::ScopeContext::Global) => crate::parsing::ScopeLevel::Global,
                    Some(crate::symbol::ScopeContext::Local { .. }) => {
                        crate::parsing::ScopeLevel::Local
                    }
                    _ => crate::parsing::ScopeLevel::Module,
                };

                context.add_symbol(symbol.name.to_string(), symbol.id, scope_level);
            }
        }

        // 3. Add visible symbols from other files in the same package
        // This is limited to avoid performance issues
        let all_symbols =
            document_index
                .get_all_symbols(5000)
                .map_err(|e| IndexError::TantivyError {
                    operation: "get_all_symbols".to_string(),
                    cause: e.to_string(),
                })?;

        for symbol in all_symbols {
            if symbol.file_id != file_id && self.is_symbol_visible_from_file(&symbol, file_id) {
                // Only add public module-level symbols from other files
                if matches!(symbol.visibility, Visibility::Public) {
                    context.add_symbol(
                        symbol.name.to_string(),
                        symbol.id,
                        crate::parsing::ScopeLevel::Global,
                    );
                }
            }
        }

        Ok(Box::new(context))
    }

    // Ruby-specific: Check if a symbol should be added to resolution context
    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;
        use crate::symbol::ScopeContext;

        // Ruby resolves classes, modules, methods, and module-level variables/constants
        let resolvable_kind = matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Class
                | SymbolKind::Module
                | SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Method
        );

        if !resolvable_kind {
            return false;
        }

        // Check scope context
        if let Some(ref scope_context) = symbol.scope_context {
            match scope_context {
                ScopeContext::Module | ScopeContext::Global => true,
                ScopeContext::Local { hoisted, .. } => {
                    // In Ruby, methods are hoisted within their scope
                    !hoisted || symbol.kind == SymbolKind::Method
                }
                ScopeContext::ClassMember => {
                    // Class/module members are resolvable through the class/module
                    true
                }
                ScopeContext::Parameter => false,
                ScopeContext::Package => true,
            }
        } else {
            // Default to resolvable for module-level symbols
            true
        }
    }

    // Ruby-specific: Handle Ruby module imports
    fn resolve_import(
        &self,
        import: &crate::parsing::Import,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // Ruby imports can be:
        // 1. require 'module_name' - absolute require
        // 2. require_relative 'path/to/file' - relative require
        // 3. require 'module::SubModule' - scoped require

        // Get the importing module path for context
        let importing_module = self.get_module_path_for_file(import.file_id);

        // Use enhanced resolution with module context
        self.resolve_import_path_with_context(
            &import.path,
            importing_module.as_deref(),
            document_index,
        )
    }

    // Ruby-specific: Check visibility based on keywords
    fn is_symbol_visible_from_file(&self, symbol: &crate::Symbol, from_file: FileId) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // Ruby uses explicit visibility keywords
        // Private symbols are not visible outside their module/class
        if matches!(symbol.visibility, Visibility::Private) {
            return false;
        }

        // Protected symbols are visible within subclasses
        // For simplicity, we allow them for now (could be refined)
        if matches!(symbol.visibility, Visibility::Module) {
            return true;
        }

        // Public symbols are always visible
        true
    }
}

// Rails-specific methods for RubyBehavior (not part of trait)
impl RubyBehavior {
    /// Build resolution context with Rails autoloading support
    ///
    /// This extends the standard resolution context with Rails-autoloaded symbols,
    /// enabling cross-file constant resolution without explicit require statements.
    pub fn build_resolution_context_with_rails(
        &self,
        file_id: FileId,
        document_index: &crate::storage::DocumentIndex,
        rails_symbol_table: &crate::parsing::ruby::RailsSymbolTable,
    ) -> crate::error::IndexResult<Box<dyn crate::parsing::ResolutionScope>> {
        use crate::parsing::resolution::GenericResolutionContext;

        // 1. Start with existing context from Issue #18 (imports)
        let mut context = self.build_resolution_context(file_id, document_index)?;

        // 2. If Rails symbol table is empty (non-Rails project), return existing context
        if rails_symbol_table.is_empty() {
            return Ok(context);
        }

        // 3. Get current file's namespace (from module_path)
        let current_namespace = self
            .get_module_path_for_file(file_id)
            .unwrap_or_else(|| String::new());

        // 4. Build namespace search list (current → parent → top-level)
        let search_namespaces = self.build_namespace_search_list(&current_namespace);

        if crate::config::is_global_debug_enabled() {
            eprintln!(
                "DEBUG: Rails resolution for file {:?}, namespace: {}, search list: {:?}",
                file_id, current_namespace, search_namespaces
            );
        }

        // 5. Add Rails autoloaded symbols to context
        for namespace in &search_namespaces {
            let constants = rails_symbol_table.get_constants_in_namespace(namespace);

            for constant_name in constants {
                // Get the short name for the symbol (last component of the constant)
                let short_name = constant_name.rsplit("::").next().unwrap_or(constant_name);

                // Look up the symbol in the document index by name
                if let Ok(symbols) = document_index.find_symbols_by_name(short_name, None) {
                    // Filter to find the symbol that matches this Rails constant
                    // We need to match by module_path to ensure we get the right one
                    for symbol in symbols {
                        // Check if the symbol's module_path matches the constant name
                        if let Some(ref module_path) = symbol.module_path {
                            if module_path.as_ref() == constant_name
                                || module_path.as_ref().ends_with(&format!("::{constant_name}"))
                            {
                                // Add to Package scope (same as imports in Issue #18)
                                if let Some(mut_context) = context
                                    .as_any_mut()
                                    .downcast_mut::<GenericResolutionContext>()
                                {
                                    mut_context.add_symbol(
                                        short_name.to_string(),
                                        symbol.id,
                                        crate::parsing::ScopeLevel::Package,
                                    );
                                }

                                if crate::config::is_global_debug_enabled() {
                                    eprintln!(
                                        "DEBUG: Added Rails constant {} (id: {:?}) to resolution context",
                                        short_name, symbol.id
                                    );
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }

        Ok(context)
    }

    /// Build namespace search list for Rails constant resolution
    ///
    /// Returns namespaces in search order: current → parent → ... → top-level
    /// Example: "Api::V1::User" → ["Api::V1::User", "Api::V1", "Api", ""]
    fn build_namespace_search_list(&self, current: &str) -> Vec<String> {
        let mut namespaces = Vec::new();

        // Add current namespace
        if !current.is_empty() {
            namespaces.push(current.to_string());
        }

        // Add parent namespaces (Api::V1::User → Api::V1, Api)
        let parts: Vec<&str> = current.split("::").collect();
        for i in (1..parts.len()).rev() {
            namespaces.push(parts[..i].join("::"));
        }

        // Add top-level (empty string represents global scope)
        namespaces.push(String::new());

        namespaces
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_module_path() {
        let behavior = RubyBehavior::new();
        assert_eq!(
            behavior.format_module_path("Module::SubModule", "function"),
            "Module::SubModule"
        );
    }

    #[test]
    fn test_parse_visibility() {
        let behavior = RubyBehavior::new();

        // Public methods (default)
        assert_eq!(behavior.parse_visibility("def foo"), Visibility::Public);
        assert_eq!(
            behavior.parse_visibility("class MyClass"),
            Visibility::Public
        );

        // Private methods
        assert_eq!(
            behavior.parse_visibility("private def internal"),
            Visibility::Private
        );
        assert_eq!(
            behavior.parse_visibility("def foo\nprivate\ndef bar"),
            Visibility::Private
        );

        // Protected methods
        assert_eq!(
            behavior.parse_visibility("protected def semi_internal"),
            Visibility::Module
        );

        // Public explicit
        assert_eq!(
            behavior.parse_visibility("public def explicit_public"),
            Visibility::Public
        );
    }

    #[test]
    fn test_module_separator() {
        let behavior = RubyBehavior::new();
        assert_eq!(behavior.module_separator(), "::");
    }

    #[test]
    fn test_supports_features() {
        let behavior = RubyBehavior::new();
        assert!(!behavior.supports_traits()); // Ruby uses mixins, not traits
        assert!(behavior.supports_inherent_methods()); // Ruby has class/module methods
    }

    #[test]
    fn test_validate_node_kinds() {
        let behavior = RubyBehavior::new();

        // Valid Ruby node kinds
        assert!(behavior.validate_node_kind("class"));
        assert!(behavior.validate_node_kind("module"));
        assert!(behavior.validate_node_kind("method"));

        // Invalid node kind
        assert!(!behavior.validate_node_kind("struct_item")); // Rust-specific
    }

    #[test]
    fn test_module_path_from_file() {
        let behavior = RubyBehavior::new();
        let root = Path::new("/project");

        // Test regular module
        let module_path = Path::new("/project/lib/package/module.rb");
        assert_eq!(
            behavior.module_path_from_file(module_path, root),
            Some("package::module".to_string())
        );

        // Test nested module
        let nested_path = Path::new("/project/lib/package/subpackage/module.rb");
        assert_eq!(
            behavior.module_path_from_file(nested_path, root),
            Some("package::subpackage::module".to_string())
        );

        // Test app directory
        let app_path = Path::new("/project/app/models/user.rb");
        assert_eq!(
            behavior.module_path_from_file(app_path, root),
            Some("models::user".to_string())
        );

        // Test rake file
        let rake_path = Path::new("/project/lib/tasks/deploy.rake");
        assert_eq!(
            behavior.module_path_from_file(rake_path, root),
            Some("tasks::deploy".to_string())
        );

        // Test without lib directory
        let no_lib_path = Path::new("/project/mymodule/myclass.rb");
        assert_eq!(
            behavior.module_path_from_file(no_lib_path, root),
            Some("mymodule::myclass".to_string())
        );
    }

    #[test]
    fn test_resolve_ruby_relative_require() {
        let behavior = RubyBehavior::new();

        // Test same directory: from Module/SubModule.rb, require helper in same dir (Module/)
        assert_eq!(
            behavior.resolve_ruby_relative_require("helper", "Module::SubModule"),
            "Module::helper"
        );

        // Test parent directory: from Module/SubModule.rb, go up one level (..) to sibling
        assert_eq!(
            behavior.resolve_ruby_relative_require("../sibling", "Module::SubModule"),
            "sibling"
        );

        // Test nested path: from Module.rb, require util/formatter
        assert_eq!(
            behavior.resolve_ruby_relative_require("util/formatter", "Module"),
            "util::formatter"
        );
    }

    #[test]
    fn test_ruby_registry_integration() {
        let registry = crate::parsing::get_registry().lock().unwrap();

        // Test 1: Check if .rb extension is recognized
        let rb_lang = registry.get_by_extension("rb");
        assert!(rb_lang.is_some(), ".rb extension should be recognized");
        assert_eq!(rb_lang.unwrap().name(), "Ruby");

        // Test 2: Check if .rake extension is recognized
        let rake_lang = registry.get_by_extension("rake");
        assert!(rake_lang.is_some(), ".rake extension should be recognized");
        assert_eq!(rake_lang.unwrap().name(), "Ruby");

        // Test 3: Check if .gemspec extension is recognized
        let gemspec_lang = registry.get_by_extension("gemspec");
        assert!(
            gemspec_lang.is_some(),
            ".gemspec extension should be recognized"
        );
        assert_eq!(gemspec_lang.unwrap().name(), "Ruby");

        // Test 4: Find Ruby language ID
        let lang_id = registry.find_language_id("ruby");
        assert!(lang_id.is_some(), "Ruby language ID should be findable");
    }

    /// Simple integration test verifying Ruby behavior works end-to-end
    ///
    /// Tests from issue #3 acceptance criteria:
    /// - File path 'lib/app/user.rb' converts to 'app::user' module path
    /// - Visibility parsing works for _private vs public names
    #[test]
    fn test_ruby_integration_end_to_end() {
        use std::path::Path;
        let behavior = RubyBehavior::new();

        // Test 1: File path to module path conversion (lib/app/user.rb → app::user)
        // Using absolute paths to simulate real project structure
        let file_path = Path::new("/project/lib/app/user.rb");
        let project_root = Path::new("/project");
        let module_path = behavior.module_path_from_file(file_path, project_root);
        assert_eq!(
            module_path,
            Some("app::user".to_string()),
            "/project/lib/app/user.rb should convert to app::user"
        );

        // Test 2: Visibility parsing for private methods (Ruby uses keywords)
        let private_method = behavior.parse_visibility("private def helper");
        assert_eq!(
            private_method,
            crate::symbol::Visibility::Private,
            "Methods with 'private' keyword should be private"
        );

        // Test 3: Visibility parsing for protected methods
        let protected_method = behavior.parse_visibility("protected def internal");
        assert_eq!(
            protected_method,
            crate::symbol::Visibility::Module,
            "Methods with 'protected' keyword should be module-level"
        );

        // Test 4: Visibility parsing for public methods (default in Ruby)
        let public_method = behavior.parse_visibility("def public_method");
        assert_eq!(
            public_method,
            crate::symbol::Visibility::Public,
            "Methods without visibility keyword should be public by default"
        );

        // Test 5: Module separator is ::
        assert_eq!(
            behavior.module_separator(),
            "::",
            "Ruby module separator should be ::"
        );

        // Test 6: Format module path (base_path + symbol_name)
        let formatted = behavior.format_module_path("User", "Authentication");
        assert_eq!(
            formatted, "User",
            "Ruby uses file paths as module paths (base_path only)"
        );
    }
}
