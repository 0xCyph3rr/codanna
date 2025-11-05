use codanna::parsing::ruby::behavior::RubyBehavior;
use codanna::parsing::ruby::parser::RubyParser;
use codanna::parsing::{LanguageBehavior, ScopeLevel};
use codanna::types::SymbolCounter;
use codanna::{FileId, Range, Symbol, SymbolId, SymbolKind, Visibility};

#[test]
fn test_ruby_module_path_resolution() {
    println!("\n=== Testing Ruby Module Path Resolution ===");

    let behavior = RubyBehavior::new();

    // Test module path formatting (Ruby uses :: separator)
    let module_path = behavior.format_module_path("Models::User", "find");
    assert_eq!(module_path, "Models::User");
    println!("Module path: {}", module_path);

    // Test module separator
    assert_eq!(behavior.module_separator(), "::");

    println!("✅ Module path resolution test passed");
}

#[test]
fn test_ruby_resolution_context() {
    println!("\n=== Testing Ruby Resolution Context ===");

    // Step 1: Create a symbol as it would exist in the index
    let find_method_id = SymbolId::new(42).unwrap();
    let mut find_method_symbol = Symbol::new(
        find_method_id,
        "find",
        SymbolKind::Method,
        FileId::new(1).unwrap(),
        Range::new(0, 0, 0, 0),
    );
    find_method_symbol.module_path = Some("Models::User".into());
    find_method_symbol.visibility = Visibility::Public;

    println!(
        "Created symbol: name='{}', module_path={:?}",
        find_method_symbol.name, find_method_symbol.module_path
    );

    // Step 2: Create resolution context
    let behavior = RubyBehavior::new();
    let mut context = behavior.create_resolution_context(FileId::new(2).unwrap());

    // Step 3: Add symbol to context by simple name
    println!("\nAdding symbol to resolution context:");
    println!("  By name: '{}' (simple lookup)", find_method_symbol.name);
    context.add_symbol(
        find_method_symbol.name.to_string(),
        find_method_id,
        ScopeLevel::Global,
    );

    // Step 4: Resolve by simple name
    let call_target = "find";
    println!("\nResolving call target: '{call_target}'");
    let resolved = context.resolve(call_target);
    println!("Resolution result: {resolved:?} (Expected: Some(SymbolId(42)))");

    assert_eq!(
        resolved,
        Some(find_method_id),
        "Should resolve '{call_target}' to SymbolId(42)"
    );

    // Step 5: Test full module path resolution
    println!("\n--- Testing full module path resolution ---");

    // Add by module_path
    if let Some(module_path) = &find_method_symbol.module_path {
        println!("Adding symbol by module_path: '{module_path}'");
        context.add_symbol(module_path.to_string(), find_method_id, ScopeLevel::Global);
    }

    // Now test that we can resolve the full path
    let full_path = "Models::User";
    println!("\nResolving full module path: '{full_path}'");
    let resolved_full = context.resolve(full_path);
    println!("Resolution result: {resolved_full:?} (Expected: Some(SymbolId(42)))");

    assert_eq!(
        resolved_full,
        Some(find_method_id),
        "Should resolve full path '{full_path}' to SymbolId(42)"
    );

    println!("\n✅ SUCCESS: Ruby resolution context works correctly!");
}

#[test]
fn test_ruby_require_resolution_simple() {
    println!("\n=== Testing Ruby require Resolution (Simple) ===");

    // Simulate: admin.rb requires user.rb and calls User.find()

    // Step 1: Parse user.rb to extract User class
    let user_code = r#"
module Models
  class User
    def self.find(id)
      new(id, "User #{id}")
    end

    def initialize(id, name)
      @id = id
      @name = name
    end
  end
end
"#;

    let mut parser = RubyParser::new().unwrap();
    let mut counter = SymbolCounter::new();
    let user_file_id = FileId::new(1).unwrap();

    let user_symbols = parser.parse(user_code, user_file_id, &mut counter);
    println!("Parsed user.rb, extracted {} symbols", user_symbols.len());

    // Find User class
    let user_class = user_symbols
        .iter()
        .find(|s| s.name.as_ref() == "User" && s.kind == SymbolKind::Class)
        .expect("Should find User class");
    println!(
        "Found User class: id={:?}, module_path={:?}",
        user_class.id, user_class.module_path
    );

    // Find find method
    let find_method = user_symbols
        .iter()
        .find(|s| s.name.as_ref() == "find" && s.kind == SymbolKind::Method)
        .expect("Should find find method");
    println!(
        "Found find method: id={:?}, module_path={:?}",
        find_method.id, find_method.module_path
    );

    // Step 2: Create resolution context for admin.rb that imports user.rb
    let behavior = RubyBehavior::new();
    let mut admin_context = behavior.create_resolution_context(FileId::new(2).unwrap());

    // Simulate adding imported symbols from user.rb
    // In a real scenario, this would be done via behavior.add_import()
    admin_context.add_symbol("User".to_string(), user_class.id, ScopeLevel::Package);
    admin_context.add_symbol("find".to_string(), find_method.id, ScopeLevel::Package);

    // Step 3: Resolve User and find method from admin.rb context
    let user_resolved = admin_context.resolve("User");
    assert_eq!(
        user_resolved,
        Some(user_class.id),
        "Should resolve User class from admin.rb"
    );

    let find_resolved = admin_context.resolve("find");
    assert_eq!(
        find_resolved,
        Some(find_method.id),
        "Should resolve find method from admin.rb"
    );

    println!("✅ SUCCESS: Ruby require resolution works!");
}

#[test]
fn test_ruby_require_relative_resolution() {
    println!("\n=== Testing Ruby require_relative Resolution ===");

    let behavior = RubyBehavior::new();

    // Test require_relative from admin.rb to user.rb (same directory)
    // From: Models::Admin
    // Require: require_relative 'user'
    // Result: Models::User

    use std::path::Path;

    // Test 1: Same directory require
    let user_path = Path::new("examples/ruby/user.rb");
    let admin_path = Path::new("examples/ruby/admin.rb");
    let project_root = Path::new("examples/ruby");

    let user_module_path = behavior
        .module_path_from_file(user_path, project_root)
        .expect("Should get module path for user.rb");
    let admin_module_path = behavior
        .module_path_from_file(admin_path, project_root)
        .expect("Should get module path for admin.rb");

    println!("user.rb module path: {}", user_module_path);
    println!("admin.rb module path: {}", admin_module_path);

    // Both should be in the same module context
    assert_eq!(user_module_path, "user");
    assert_eq!(admin_module_path, "admin");

    // Test 2: Import matching
    // Simulate: require_relative 'user' from admin.rb
    let import_path = "user";
    let symbol_module_path = "user";

    let matches = behavior.import_matches_symbol(import_path, symbol_module_path, Some("admin"));
    assert!(
        matches,
        "require_relative 'user' should match 'user' module path"
    );

    println!("✅ SUCCESS: Ruby require_relative resolution works!");
}

#[test]
fn test_ruby_cross_file_symbol_lookup() {
    println!("\n=== Testing Ruby Cross-File Symbol Lookup ===");

    // This test simulates the scenario from admin.rb which requires user.rb

    // Step 1: Parse user.rb
    let user_code = r#"
module Models
  class User
    attr_reader :id, :name

    def self.find(id)
      new(id, "User #{id}")
    end

    def initialize(id, name)
      @id = id
      @name = name
    end

    def valid?
      !@name.nil?
    end
  end
end
"#;

    let mut parser = RubyParser::new().unwrap();
    let mut counter = SymbolCounter::new();
    let user_file_id = FileId::new(1).unwrap();

    let user_symbols = parser.parse(user_code, user_file_id, &mut counter);
    println!("Parsed user.rb: {} symbols", user_symbols.len());

    // Step 2: Parse admin.rb
    let admin_code = r#"
require_relative 'user'

module Models
  class Admin < User
    def self.find(id)
      admin = super(id)
      admin.upgrade_to_admin
      admin
    end

    def initialize(id, name)
      super(id, name)
      @role = "admin"
    end

    def upgrade_to_admin
      @role = "admin"
    end
  end
end
"#;

    let admin_file_id = FileId::new(2).unwrap();
    let admin_symbols = parser.parse(admin_code, admin_file_id, &mut counter);
    println!("Parsed admin.rb: {} symbols", admin_symbols.len());

    // Step 3: Verify Admin class inherits from User
    let admin_class = admin_symbols
        .iter()
        .find(|s| s.name.as_ref() == "Admin" && s.kind == SymbolKind::Class)
        .expect("Should find Admin class");

    // Check signature includes inheritance
    assert!(
        admin_class
            .signature
            .as_ref()
            .map(|s| s.as_ref().contains("< User"))
            .unwrap_or(false),
        "Admin should inherit from User"
    );

    // Step 4: Create a resolution context that includes both files
    let behavior = RubyBehavior::new();
    let mut context = behavior.create_resolution_context(admin_file_id);

    // Add User symbols to Admin's context (simulating require_relative)
    for symbol in &user_symbols {
        if matches!(symbol.kind, SymbolKind::Class | SymbolKind::Method) {
            context.add_symbol(symbol.name.to_string(), symbol.id, ScopeLevel::Package);
        }
    }

    // Add Admin's own symbols
    for symbol in &admin_symbols {
        if matches!(symbol.kind, SymbolKind::Class | SymbolKind::Method) {
            context.add_symbol(symbol.name.to_string(), symbol.id, ScopeLevel::Module);
        }
    }

    // Step 5: Verify we can resolve User from Admin's context
    let user_class = user_symbols
        .iter()
        .find(|s| s.name.as_ref() == "User")
        .expect("Should find User class");

    let resolved_user = context.resolve("User");
    assert_eq!(
        resolved_user,
        Some(user_class.id),
        "Admin should be able to resolve User class"
    );

    println!("✅ SUCCESS: Cross-file symbol lookup works!");
}

#[test]
fn test_ruby_module_path_from_file() {
    println!("\n=== Testing Ruby Module Path from File ===");

    let behavior = RubyBehavior::new();
    use std::path::Path;

    // Test 1: lib/ directory
    let project_root = Path::new("/project");
    let lib_path = Path::new("/project/lib/models/user.rb");

    let module_path = behavior.module_path_from_file(lib_path, project_root);
    assert_eq!(module_path, Some("models::user".to_string()));
    println!("lib/models/user.rb -> {:?}", module_path);

    // Test 2: app/ directory (Rails convention)
    let app_path = Path::new("/project/app/models/user.rb");
    let module_path = behavior.module_path_from_file(app_path, project_root);
    assert_eq!(module_path, Some("models::user".to_string()));
    println!("app/models/user.rb -> {:?}", module_path);

    // Test 3: Nested modules
    let nested_path = Path::new("/project/lib/authentication/oauth/provider.rb");
    let module_path = behavior.module_path_from_file(nested_path, project_root);
    assert_eq!(
        module_path,
        Some("authentication::oauth::provider".to_string())
    );
    println!("lib/authentication/oauth/provider.rb -> {:?}", module_path);

    // Test 4: rake file
    let rake_path = Path::new("/project/lib/tasks/deploy.rake");
    let module_path = behavior.module_path_from_file(rake_path, project_root);
    assert_eq!(module_path, Some("tasks::deploy".to_string()));
    println!("lib/tasks/deploy.rake -> {:?}", module_path);

    println!("✅ Module path from file test passed");
}

#[test]
fn test_ruby_visibility_cross_module() {
    println!("\n=== Testing Ruby Visibility Across Modules ===");

    let behavior = RubyBehavior::new();

    // Test visibility parsing
    assert_eq!(behavior.parse_visibility("def foo"), Visibility::Public);
    assert_eq!(
        behavior.parse_visibility("private def foo"),
        Visibility::Private
    );
    assert_eq!(
        behavior.parse_visibility("protected def foo"),
        Visibility::Module
    );
    assert_eq!(
        behavior.parse_visibility("public def foo"),
        Visibility::Public
    );

    // Test that visibility affects resolution
    let file1 = FileId::new(1).unwrap();
    let file2 = FileId::new(2).unwrap();

    // Public symbol should be visible from other files
    let mut public_symbol = Symbol::new(
        SymbolId::new(1).unwrap(),
        "public_method",
        SymbolKind::Method,
        file1,
        Range::new(0, 0, 0, 0),
    );
    public_symbol.visibility = Visibility::Public;

    assert!(
        behavior.is_symbol_visible_from_file(&public_symbol, file2),
        "Public symbols should be visible from other files"
    );

    // Private symbol should NOT be visible from other files
    let mut private_symbol = Symbol::new(
        SymbolId::new(2).unwrap(),
        "private_method",
        SymbolKind::Method,
        file1,
        Range::new(0, 0, 0, 0),
    );
    private_symbol.visibility = Visibility::Private;

    assert!(
        !behavior.is_symbol_visible_from_file(&private_symbol, file2),
        "Private symbols should NOT be visible from other files"
    );

    // Protected (Module-level) symbol should be visible
    let mut protected_symbol = Symbol::new(
        SymbolId::new(3).unwrap(),
        "protected_method",
        SymbolKind::Method,
        file1,
        Range::new(0, 0, 0, 0),
    );
    protected_symbol.visibility = Visibility::Module;

    assert!(
        behavior.is_symbol_visible_from_file(&protected_symbol, file2),
        "Protected symbols should be visible (for simplicity)"
    );

    println!("✅ Visibility cross-module test passed");
}

#[test]
fn test_ruby_import_matching() {
    println!("\n=== Testing Ruby Import Matching ===");

    let behavior = RubyBehavior::new();

    // Test 1: Exact match
    assert!(
        behavior.import_matches_symbol("Models::User", "Models::User", None),
        "Exact match should work"
    );

    // Test 2: Suffix match (partial import)
    assert!(
        behavior.import_matches_symbol("User", "Models::User", Some("Models")),
        "Should match suffix"
    );

    // Test 3: Relative import (require_relative)
    assert!(
        behavior.import_matches_symbol("./user", "Models::user", Some("Models::Admin")),
        "Should handle relative imports"
    );

    // Test 4: Non-matching
    assert!(
        !behavior.import_matches_symbol("Admin", "Models::User", Some("Models")),
        "Different symbols should not match"
    );

    println!("✅ Import matching test passed");
}
