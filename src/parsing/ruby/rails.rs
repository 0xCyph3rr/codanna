//! Rails autoloading support for Ruby
//!
//! This module implements Rails (Zeitwerk) autoloading conventions to enable
//! cross-file constant resolution without explicit require statements.

use crate::IndexResult;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Rails load path configuration
#[derive(Debug, Clone)]
pub struct RailsLoadPath {
    /// Base directory (e.g., "app/models")
    pub base_dir: PathBuf,
    /// Namespace prefix (None for top-level)
    pub namespace_prefix: Option<String>,
}

impl RailsLoadPath {
    /// Create a new Rails load path
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            namespace_prefix: None,
        }
    }

    /// Create a Rails load path with a namespace prefix
    pub fn with_namespace<P: AsRef<Path>>(base_dir: P, namespace: String) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            namespace_prefix: Some(namespace),
        }
    }
}

/// Detects Rails projects and discovers autoload paths
pub struct RailsProjectDetector {
    project_root: PathBuf,
}

impl RailsProjectDetector {
    /// Create a new detector for the given project root
    pub fn new<P: AsRef<Path>>(project_root: P) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
        }
    }

    /// Detect if directory is a Rails project
    ///
    /// Primary indicator: config/application.rb with Rails::Application
    /// Secondary: app/ directory + Gemfile with rails gem
    pub fn is_rails_project(&self) -> bool {
        // Primary check: config/application.rb with Rails::Application
        let app_rb = self.project_root.join("config/application.rb");
        if app_rb.exists() {
            if let Ok(contents) = fs::read_to_string(&app_rb) {
                if contents.contains("Rails::Application") {
                    return true;
                }
            }
        }

        // Fallback: Check for app/ directory + Gemfile with rails gem
        let has_app_dir = self.project_root.join("app").is_dir();
        let gemfile = self.project_root.join("Gemfile");
        if has_app_dir && gemfile.exists() {
            if let Ok(contents) = fs::read_to_string(&gemfile) {
                return contents.contains("gem 'rails'") || contents.contains("gem \"rails\"");
            }
        }

        false
    }

    /// Discover all Rails autoload paths
    ///
    /// Returns standard Rails autoload directories that are commonly used
    /// by Zeitwerk for autoloading constants.
    pub fn discover_load_paths(&self) -> Vec<RailsLoadPath> {
        vec![
            RailsLoadPath::new(self.project_root.join("app/models")),
            RailsLoadPath::new(self.project_root.join("app/controllers")),
            RailsLoadPath::new(self.project_root.join("app/decorators")),
            RailsLoadPath::new(self.project_root.join("app/helpers")),
            RailsLoadPath::new(self.project_root.join("app/services")),
            RailsLoadPath::new(self.project_root.join("app/jobs")),
            RailsLoadPath::new(self.project_root.join("app/mailers")),
            RailsLoadPath::new(self.project_root.join("app/models/concerns")),
            RailsLoadPath::new(self.project_root.join("app/controllers/concerns")),
            // Legacy patterns (e.g., guliveo uses app/models/lib)
            RailsLoadPath::new(self.project_root.join("app/models/lib")),
            // Standard lib/ directory
            RailsLoadPath::new(self.project_root.join("lib")),
        ]
    }
}

/// Rails inflector for file path ↔ constant name conversion
pub struct RailsInflector;

impl RailsInflector {
    /// Convert file path to constant name using Rails conventions
    ///
    /// Examples:
    /// - "app/models/user.rb" → "User"
    /// - "app/models/lib/url_formatter.rb" → "UrlFormatter"
    /// - "app/models/api/v1/user.rb" → "Api::V1::User"
    ///
    /// Returns None if the file is not within the load path or has invalid structure
    pub fn file_path_to_constant(
        file_path: &Path,
        load_path: &RailsLoadPath,
    ) -> Option<String> {
        // 1. Strip load path base directory
        let relative = file_path.strip_prefix(&load_path.base_dir).ok()?;

        // 2. Strip .rb extension
        let without_ext = relative.with_extension("");

        // 3. Convert path components to module names
        let components: Vec<String> = without_ext
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .map(|s| Self::underscore_to_camelcase(s))
            .collect();

        if components.is_empty() {
            return None;
        }

        // 4. Join with "::" for nested modules
        let constant = components.join("::");

        // 5. Apply namespace prefix if present
        if let Some(prefix) = &load_path.namespace_prefix {
            Some(format!("{prefix}::{constant}"))
        } else {
            Some(constant)
        }
    }

    /// Convert underscore_case to CamelCase
    ///
    /// Examples:
    /// - "url_formatter" → "UrlFormatter"
    /// - "user" → "User"
    /// - "api" → "Api"
    fn underscore_to_camelcase(s: &str) -> String {
        s.split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect()
    }

    /// Convert CamelCase to underscore_case
    ///
    /// Examples:
    /// - "UrlFormatter" → "url_formatter"
    /// - "Api::V1::User" → "api/v1/user"
    fn camelcase_to_underscore(s: &str) -> String {
        // First replace :: with / to handle namespaces
        let with_slashes = s.replace("::", "/");

        // Then convert each component from CamelCase to underscore_case
        let mut result = String::new();
        let mut prev_was_upper = false;

        for (i, c) in with_slashes.chars().enumerate() {
            if c == '/' {
                result.push(c);
                prev_was_upper = false;
            } else if c.is_uppercase() {
                if i > 0 && !prev_was_upper && result.chars().last() != Some('/') {
                    result.push('_');
                }
                result.push(c.to_lowercase().next().unwrap());
                prev_was_upper = true;
            } else {
                result.push(c);
                prev_was_upper = false;
            }
        }

        result
    }

    /// Convert constant name to possible file paths
    ///
    /// Examples:
    /// - "UrlFormatter" → ["url_formatter.rb"]
    /// - "Api::V1::User" → ["api/v1/user.rb"]
    ///
    /// Returns relative path components that need to be joined with each load path
    pub fn constant_to_file_path_component(constant: &str) -> String {
        format!("{}.rb", Self::camelcase_to_underscore(constant))
    }
}

/// Rails symbol table for O(1) constant resolution
///
/// This table is built once during indexing and caches the mapping between
/// Rails constants and their file locations, following Zeitwerk conventions.
pub struct RailsSymbolTable {
    /// Map: constant_name → file_path
    /// Example: "UrlFormatter" → "app/models/lib/url_formatter.rb"
    constant_to_file: HashMap<String, PathBuf>,

    /// Map: namespace → Vec<constant_names>
    /// Example: "Api::V1" → ["User", "Post", "Comment"]
    /// Used for building resolution context with namespace awareness
    namespace_index: HashMap<String, Vec<String>>,

    /// Map: file_path → constant_name
    /// Example: "app/models/lib/url_formatter.rb" → "UrlFormatter"
    file_to_constant: HashMap<PathBuf, String>,

    /// Detected load paths
    load_paths: Vec<RailsLoadPath>,

    /// Project root for path resolution
    project_root: PathBuf,
}

impl RailsSymbolTable {
    /// Create an empty Rails symbol table
    pub fn empty() -> Self {
        Self {
            constant_to_file: HashMap::new(),
            namespace_index: HashMap::new(),
            file_to_constant: HashMap::new(),
            load_paths: Vec::new(),
            project_root: PathBuf::new(),
        }
    }

    /// Build symbol table by scanning Rails project
    ///
    /// This scans all Ruby files in Rails autoload paths and builds the
    /// constant → file mapping using Zeitwerk conventions.
    pub fn build(project_root: &Path) -> IndexResult<Self> {
        let detector = RailsProjectDetector::new(project_root);

        if !detector.is_rails_project() {
            // Not a Rails project, return empty table
            eprintln!("DEBUG: Not a Rails project, skipping Rails autoloading support");
            return Ok(Self::empty());
        }

        eprintln!("DEBUG: Detected Rails project, building symbol table for autoloading");

        let load_paths = detector.discover_load_paths();
        let mut table = Self {
            constant_to_file: HashMap::new(),
            namespace_index: HashMap::new(),
            file_to_constant: HashMap::new(),
            load_paths: load_paths.clone(),
            project_root: project_root.to_path_buf(),
        };

        // Scan all Ruby files in load paths
        let mut files_scanned = 0;
        let mut constants_mapped = 0;

        for load_path in &load_paths {
            if !load_path.base_dir.exists() {
                continue;
            }

            eprintln!(
                "DEBUG: Scanning Rails load path: {}",
                load_path.base_dir.display()
            );

            // Walk directory tree
            for entry in WalkDir::new(&load_path.base_dir)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("rb") {
                    files_scanned += 1;

                    if let Some(constant) =
                        RailsInflector::file_path_to_constant(entry.path(), load_path)
                    {
                        // Make path relative to project root for storage
                        if let Ok(relative_path) = entry.path().strip_prefix(project_root) {
                            table.register(constant.clone(), relative_path.to_path_buf());
                            constants_mapped += 1;

                            if crate::config::is_global_debug_enabled() {
                                eprintln!(
                                    "DEBUG: Mapped Rails constant: {} → {}",
                                    constant,
                                    relative_path.display()
                                );
                            }
                        }
                    }
                }
            }
        }

        eprintln!(
            "DEBUG: Rails symbol table built: {} files scanned, {} constants mapped",
            files_scanned, constants_mapped
        );

        Ok(table)
    }

    /// Register a constant-to-file mapping
    fn register(&mut self, constant: String, file_path: PathBuf) {
        // Update constant → file mapping
        self.constant_to_file
            .insert(constant.clone(), file_path.clone());

        // Update file → constant mapping
        self.file_to_constant.insert(file_path, constant.clone());

        // Update namespace index
        let namespace = self.extract_namespace(&constant);
        self.namespace_index
            .entry(namespace)
            .or_default()
            .push(constant);
    }

    /// Extract namespace from constant (Api::V1::User → Api::V1)
    /// Returns empty string for top-level constants (User → "")
    fn extract_namespace(&self, constant: &str) -> String {
        constant
            .rsplitn(2, "::")
            .nth(1)
            .unwrap_or("")
            .to_string()
    }

    /// Get file path for constant
    pub fn get_file_for_constant(&self, constant: &str) -> Option<&PathBuf> {
        self.constant_to_file.get(constant)
    }

    /// Get constant name for file
    pub fn get_constant_for_file(&self, file_path: &Path) -> Option<&String> {
        self.file_to_constant.get(file_path)
    }

    /// Get all constants in a namespace (for resolution context)
    ///
    /// Returns all constants that belong to the given namespace.
    /// For top-level namespace (empty string ""), returns top-level constants.
    pub fn get_constants_in_namespace(&self, namespace: &str) -> Vec<&String> {
        self.namespace_index
            .get(namespace)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Check if this is an empty table (no Rails project detected)
    pub fn is_empty(&self) -> bool {
        self.constant_to_file.is_empty()
    }

    /// Get project root
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Find constant by file and name in the symbol table
    ///
    /// This helper resolves a constant reference to its defining file path,
    /// which can then be used to look up the actual SymbolId in the document index.
    pub fn resolve_constant(&self, constant_name: &str) -> Option<&PathBuf> {
        self.get_file_for_constant(constant_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper to create a test Rails project structure
    fn create_test_rails_project() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create config/application.rb with Rails::Application
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(
            root.join("config/application.rb"),
            "class Application < Rails::Application\nend",
        )
        .unwrap();

        // Create Gemfile with rails gem
        fs::write(root.join("Gemfile"), "gem 'rails'").unwrap();

        // Create app directory structure
        fs::create_dir_all(root.join("app/models")).unwrap();
        fs::create_dir_all(root.join("app/models/lib")).unwrap();
        fs::create_dir_all(root.join("app/decorators")).unwrap();
        fs::create_dir_all(root.join("app/models/api/v1")).unwrap();

        // Create test files
        fs::write(root.join("app/models/user.rb"), "class User\nend").unwrap();
        fs::write(
            root.join("app/models/lib/url_formatter.rb"),
            "module UrlFormatter\nend",
        )
        .unwrap();
        fs::write(
            root.join("app/decorators/contact_decorator.rb"),
            "class ContactDecorator\nend",
        )
        .unwrap();
        fs::write(
            root.join("app/models/api/v1/user.rb"),
            "module Api\n  module V1\n    class User\n    end\n  end\nend",
        )
        .unwrap();

        temp_dir
    }

    #[test]
    fn test_rails_project_detection() {
        let temp_dir = create_test_rails_project();
        let detector = RailsProjectDetector::new(temp_dir.path());

        assert!(
            detector.is_rails_project(),
            "Should detect Rails project via config/application.rb"
        );
    }

    #[test]
    fn test_rails_project_detection_gemfile_fallback() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // No config/application.rb, but has app/ + Gemfile
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(root.join("Gemfile"), "gem 'rails'").unwrap();

        let detector = RailsProjectDetector::new(root);
        assert!(
            detector.is_rails_project(),
            "Should detect Rails project via app/ + Gemfile fallback"
        );
    }

    #[test]
    fn test_non_rails_project_detection() {
        let temp_dir = TempDir::new().unwrap();
        let detector = RailsProjectDetector::new(temp_dir.path());

        assert!(
            !detector.is_rails_project(),
            "Should not detect non-Rails project"
        );
    }

    #[test]
    fn test_file_to_constant_simple() {
        let load_path = RailsLoadPath::new("app/models");
        let file_path = Path::new("app/models/user.rb");

        let constant = RailsInflector::file_path_to_constant(file_path, &load_path);
        assert_eq!(constant, Some("User".to_string()));
    }

    #[test]
    fn test_file_to_constant_nested() {
        let load_path = RailsLoadPath::new("app/models");
        let file_path = Path::new("app/models/api/v1/user.rb");

        let constant = RailsInflector::file_path_to_constant(file_path, &load_path);
        assert_eq!(constant, Some("Api::V1::User".to_string()));
    }

    #[test]
    fn test_file_to_constant_underscore() {
        let load_path = RailsLoadPath::new("app/models/lib");
        let file_path = Path::new("app/models/lib/url_formatter.rb");

        let constant = RailsInflector::file_path_to_constant(file_path, &load_path);
        assert_eq!(constant, Some("UrlFormatter".to_string()));
    }

    #[test]
    fn test_underscore_to_camelcase() {
        assert_eq!(
            RailsInflector::underscore_to_camelcase("url_formatter"),
            "UrlFormatter"
        );
        assert_eq!(RailsInflector::underscore_to_camelcase("user"), "User");
        assert_eq!(RailsInflector::underscore_to_camelcase("api"), "Api");
        assert_eq!(
            RailsInflector::underscore_to_camelcase("contact_decorator"),
            "ContactDecorator"
        );
    }

    #[test]
    fn test_camelcase_to_underscore() {
        assert_eq!(
            RailsInflector::camelcase_to_underscore("UrlFormatter"),
            "url_formatter"
        );
        assert_eq!(RailsInflector::camelcase_to_underscore("User"), "user");
        assert_eq!(
            RailsInflector::camelcase_to_underscore("Api::V1::User"),
            "api/v1/user"
        );
    }

    #[test]
    fn test_constant_to_file_path_component() {
        assert_eq!(
            RailsInflector::constant_to_file_path_component("UrlFormatter"),
            "url_formatter.rb"
        );
        assert_eq!(
            RailsInflector::constant_to_file_path_component("Api::V1::User"),
            "api/v1/user.rb"
        );
    }

    #[test]
    fn test_rails_symbol_table_build() {
        let temp_dir = create_test_rails_project();
        let table = RailsSymbolTable::build(temp_dir.path()).unwrap();

        assert!(!table.is_empty(), "Symbol table should not be empty");

        // Check that constants were mapped correctly
        let user_file = table.get_file_for_constant("User");
        assert!(user_file.is_some(), "User constant should be mapped");
        assert!(
            user_file.unwrap().to_str().unwrap().contains("app/models/user.rb"),
            "User should map to app/models/user.rb"
        );

        let formatter_file = table.get_file_for_constant("UrlFormatter");
        assert!(
            formatter_file.is_some(),
            "UrlFormatter constant should be mapped"
        );
        assert!(
            formatter_file.unwrap().to_str().unwrap().contains("app/models/lib/url_formatter.rb"),
            "UrlFormatter should map to app/models/lib/url_formatter.rb"
        );

        let nested_file = table.get_file_for_constant("Api::V1::User");
        assert!(
            nested_file.is_some(),
            "Api::V1::User constant should be mapped"
        );
        assert!(
            nested_file.unwrap().to_str().unwrap().contains("app/models/api/v1/user.rb"),
            "Api::V1::User should map to app/models/api/v1/user.rb"
        );
    }

    #[test]
    fn test_namespace_extraction() {
        let table = RailsSymbolTable::empty();

        assert_eq!(table.extract_namespace("User"), "");
        assert_eq!(table.extract_namespace("Api::User"), "Api");
        assert_eq!(table.extract_namespace("Api::V1::User"), "Api::V1");
    }

    #[test]
    fn test_namespace_index() {
        let temp_dir = create_test_rails_project();
        let table = RailsSymbolTable::build(temp_dir.path()).unwrap();

        // Get top-level constants (empty namespace)
        let top_level = table.get_constants_in_namespace("");
        assert!(
            !top_level.is_empty(),
            "Should have top-level constants (User, UrlFormatter, ContactDecorator)"
        );

        // Get nested constants (Api::V1 namespace)
        let api_v1 = table.get_constants_in_namespace("Api::V1");
        assert!(
            !api_v1.is_empty(),
            "Should have Api::V1::User in Api::V1 namespace"
        );
        assert!(
            api_v1.contains(&&"Api::V1::User".to_string()),
            "Api::V1 namespace should contain Api::V1::User"
        );
    }

    #[test]
    fn test_empty_table() {
        let temp_dir = TempDir::new().unwrap();
        let table = RailsSymbolTable::build(temp_dir.path()).unwrap();

        assert!(table.is_empty(), "Non-Rails project should have empty table");
        assert_eq!(
            table.get_constants_in_namespace("").len(),
            0,
            "Empty table should have no constants"
        );
    }
}
