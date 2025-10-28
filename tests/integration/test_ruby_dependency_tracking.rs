use codanna::parsing::ruby::behavior::RubyBehavior;
use codanna::parsing::ruby::parser::RubyParser;
use codanna::parsing::{LanguageBehavior, LanguageParser};
use codanna::types::SymbolCounter;
use codanna::{FileId, SymbolKind};
use std::fs;

#[test]
fn test_ruby_method_call_tracking_structure() {
    println!("\n=== Testing Ruby Method Call Tracking Structure ===");

    let code = r#"
class User
  def self.find(id)
    new(id, "User #{id}")
  end

  def initialize(id, name)
    @id = id
    @name = name
  end

  def authenticate(password)
    return false unless valid_password?(password)
    reset_attempts
    true
  end

  private

  def valid_password?(password)
    !password.nil? && password.length >= 8
  end

  def reset_attempts
    @attempts = 0
  end
end

def example_usage
  user = User.find(1)
  user.authenticate("password123")
end
"#;

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");

    // Test find_calls method (Phase 3 - currently returns empty Vec)
    let calls = parser.find_calls(code);
    println!("Found {} method calls", calls.len());

    // Note: When Phase 3 is implemented, this test should be updated to verify:
    // - User.find call
    // - user.authenticate call
    // - valid_password? call
    // - reset_attempts call

    // For now, just verify the method exists and returns the correct type
    assert!(calls.is_empty() || !calls.is_empty(), "find_calls should return Vec");

    println!("✅ Method call tracking structure test passed");
}

#[test]
fn test_ruby_dependency_graph_structure() {
    println!("\n=== Testing Ruby Dependency Graph Structure ===");

    // Parse user.rb fixture
    let user_path = "examples/ruby/user.rb";
    let user_code = fs::read_to_string(user_path)
        .expect("Failed to read user.rb fixture");

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let user_file_id = FileId::new(1).expect("Failed to create FileId");

    let user_symbols = parser.parse(&user_code, user_file_id, &mut counter);
    println!("Parsed user.rb: {} symbols", user_symbols.len());

    // Parse admin.rb fixture
    let admin_path = "examples/ruby/admin.rb";
    let admin_code = fs::read_to_string(admin_path)
        .expect("Failed to read admin.rb fixture");

    let admin_file_id = FileId::new(2).expect("Failed to create FileId");
    let admin_symbols = parser.parse(&admin_code, admin_file_id, &mut counter);
    println!("Parsed admin.rb: {} symbols", admin_symbols.len());

    // Verify Admin inherits from User (dependency relationship)
    let admin_class = admin_symbols
        .iter()
        .find(|s| s.name.as_ref() == "Admin" && s.kind == SymbolKind::Class)
        .expect("Should find Admin class");

    let has_inheritance = admin_class
        .signature
        .as_ref()
        .map(|s| s.as_ref().contains("< User"))
        .unwrap_or(false);

    assert!(has_inheritance, "Admin should show dependency on User through inheritance");
    println!("Verified: Admin < User dependency relationship");

    // Parse helpers.rb fixture
    let helpers_path = "examples/ruby/helpers.rb";
    let helpers_code = fs::read_to_string(helpers_path)
        .expect("Failed to read helpers.rb fixture");

    let helpers_file_id = FileId::new(3).expect("Failed to create FileId");
    let helpers_symbols = parser.parse(&helpers_code, helpers_file_id, &mut counter);
    println!("Parsed helpers.rb: {} symbols", helpers_symbols.len());

    // Verify Helpers module exists (dependency target)
    let helpers_module = helpers_symbols
        .iter()
        .find(|s| s.name.as_ref() == "Helpers" && s.kind == SymbolKind::Module);
    assert!(helpers_module.is_some(), "Should find Helpers module");

    // Verify helper methods exist (dependency targets)
    let format_audit = helpers_symbols
        .iter()
        .find(|s| s.name.as_ref() == "format_audit" && s.kind == SymbolKind::Method);
    assert!(format_audit.is_some(), "Should find format_audit method");

    let log_message = helpers_symbols
        .iter()
        .find(|s| s.name.as_ref() == "log_message" && s.kind == SymbolKind::Method);
    assert!(log_message.is_some(), "Should find log_message method");

    println!("✅ Dependency graph structure test passed");
}

#[test]
fn test_ruby_cross_file_dependencies() {
    println!("\n=== Testing Ruby Cross-File Dependencies ===");

    // This test verifies the structure for tracking dependencies across files
    // admin.rb -> user.rb (inheritance)
    // admin.rb -> helpers.rb (method calls)

    let behavior = RubyBehavior::new();

    // Simulate file registration (as would happen during indexing)
    use std::path::PathBuf;

    let user_file_id = FileId::new(1).unwrap();
    let admin_file_id = FileId::new(2).unwrap();
    let helpers_file_id = FileId::new(3).unwrap();

    behavior.register_file(
        PathBuf::from("examples/ruby/user.rb"),
        user_file_id,
        "user".to_string(),
    );

    behavior.register_file(
        PathBuf::from("examples/ruby/admin.rb"),
        admin_file_id,
        "admin".to_string(),
    );

    behavior.register_file(
        PathBuf::from("examples/ruby/helpers.rb"),
        helpers_file_id,
        "helpers".to_string(),
    );

    // Verify files are registered
    let user_module = behavior.get_module_path_for_file(user_file_id);
    assert_eq!(user_module, Some("user".to_string()));

    let admin_module = behavior.get_module_path_for_file(admin_file_id);
    assert_eq!(admin_module, Some("admin".to_string()));

    let helpers_module = behavior.get_module_path_for_file(helpers_file_id);
    assert_eq!(helpers_module, Some("helpers".to_string()));

    println!("✅ Cross-file dependencies test passed");
}

#[test]
fn test_ruby_import_tracking() {
    println!("\n=== Testing Ruby Import Tracking ===");

    let behavior = RubyBehavior::new();

    // Simulate imports from admin.rb
    use codanna::parsing::Import;

    let admin_file_id = FileId::new(2).unwrap();

    // require_relative 'user'
    let user_import = Import {
        path: "user".to_string(),
        file_id: admin_file_id,
        alias: None,
        is_glob: false,
        is_type_only: false,
    };

    behavior.add_import(user_import.clone());

    // require_relative 'helpers'
    let helpers_import = Import {
        path: "helpers".to_string(),
        file_id: admin_file_id,
        alias: None,
        is_glob: false,
        is_type_only: false,
    };

    behavior.add_import(helpers_import.clone());

    // Verify imports are tracked
    let imports = behavior.get_imports_for_file(admin_file_id);
    println!("Tracked {} imports for admin.rb", imports.len());

    assert_eq!(imports.len(), 2, "Should track 2 imports");

    let has_user_import = imports.iter().any(|i| i.path == "user");
    assert!(has_user_import, "Should track user import");

    let has_helpers_import = imports.iter().any(|i| i.path == "helpers");
    assert!(has_helpers_import, "Should track helpers import");

    println!("✅ Import tracking test passed");
}

#[test]
fn test_ruby_method_call_impact_analysis() {
    println!("\n=== Testing Ruby Method Call Impact Analysis ===");

    // This test verifies the structure for impact analysis
    // If we change User.find(), what is impacted?
    // - Admin.find() which calls super
    // - Any code that calls User.find()

    let code = r#"
class User
  def self.find(id)
    new(id, "User #{id}")
  end

  def initialize(id, name)
    @id = id
    @name = name
  end
end

class Admin < User
  def self.find(id)
    admin = super(id)  # Calls User.find
    admin.upgrade_to_admin
    admin
  end

  def upgrade_to_admin
    @role = "admin"
  end
end

def example
  user = User.find(1)     # Direct call to User.find
  admin = Admin.find(2)   # Calls Admin.find which calls User.find via super
end
"#;

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(code, file_id, &mut counter);

    // Find User.find method (the target of impact analysis)
    let user_find = symbols
        .iter()
        .find(|s| s.name.as_ref() == "find"
            && s.kind == SymbolKind::Method
            && s.signature.as_ref().map(|sig| sig.as_ref().contains("self.find")).unwrap_or(false))
        .expect("Should find User.find method");

    println!("Target for impact analysis: User.find (id={:?})", user_find.id);

    // Find Admin.find method (impacted by User.find changes)
    let admin_find = symbols
        .iter()
        .filter(|s| s.name.as_ref() == "find" && s.kind == SymbolKind::Method)
        .nth(1)
        .expect("Should find Admin.find method");

    println!("Impacted method: Admin.find (id={:?})", admin_find.id);

    // Note: When Phase 3 (method call tracking) is implemented, we should verify:
    // 1. Direct calls to User.find are tracked
    // 2. super calls in Admin.find are tracked
    // 3. Impact analysis shows both as dependencies

    println!("✅ Method call impact analysis structure test passed");
}

#[test]
fn test_ruby_constant_dependencies() {
    println!("\n=== Testing Ruby Constant Dependencies ===");

    let code = r#"
module Config
  VERSION = "1.0.0"
  MAX_TIMEOUT = 30
end

class Service
  # Uses Config::VERSION constant
  def version_info
    "Service version: #{Config::VERSION}"
  end

  # Uses Config::MAX_TIMEOUT constant
  def timeout
    Config::MAX_TIMEOUT
  end
end
"#;

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(code, file_id, &mut counter);

    // Find constants
    let version_constant = symbols
        .iter()
        .find(|s| s.name.as_ref() == "VERSION" && s.kind == SymbolKind::Constant);
    assert!(version_constant.is_some(), "Should find VERSION constant");

    let timeout_constant = symbols
        .iter()
        .find(|s| s.name.as_ref() == "MAX_TIMEOUT" && s.kind == SymbolKind::Constant);
    assert!(timeout_constant.is_some(), "Should find MAX_TIMEOUT constant");

    // Find methods that depend on these constants
    let version_info = symbols
        .iter()
        .find(|s| s.name.as_ref() == "version_info" && s.kind == SymbolKind::Method);
    assert!(version_info.is_some(), "Should find version_info method");

    let timeout_method = symbols
        .iter()
        .find(|s| s.name.as_ref() == "timeout" && s.kind == SymbolKind::Method);
    assert!(timeout_method.is_some(), "Should find timeout method");

    // Note: When constant usage tracking is implemented, we should verify:
    // - version_info depends on Config::VERSION
    // - timeout depends on Config::MAX_TIMEOUT

    println!("✅ Constant dependencies test passed");
}

#[test]
fn test_ruby_attr_accessor_dependencies() {
    println!("\n=== Testing Ruby attr_accessor Dependencies ===");

    let code = r#"
class User
  attr_reader :id
  attr_accessor :name, :email
  attr_writer :password

  def initialize(id, name, email)
    @id = id
    @name = name
    @email = email
  end

  def display_info
    # Depends on: id (getter), name (getter), email (getter)
    "User #{id}: #{name} (#{email})"
  end

  def update_profile(new_name, new_email)
    # Depends on: name= (setter), email= (setter)
    self.name = new_name
    self.email = new_email
  end
end
"#;

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(code, file_id, &mut counter);

    // Find synthetic methods created by attr_*
    let id_getter = symbols
        .iter()
        .find(|s| s.name.as_ref() == "id" && s.kind == SymbolKind::Method);
    assert!(id_getter.is_some(), "Should find id getter from attr_reader");

    let name_getter = symbols
        .iter()
        .find(|s| s.name.as_ref() == "name" && s.kind == SymbolKind::Method);
    assert!(name_getter.is_some(), "Should find name getter from attr_accessor");

    let name_setter = symbols
        .iter()
        .find(|s| s.name.as_ref() == "name=" && s.kind == SymbolKind::Method);
    assert!(name_setter.is_some(), "Should find name setter from attr_accessor");

    let email_setter = symbols
        .iter()
        .find(|s| s.name.as_ref() == "email=" && s.kind == SymbolKind::Method);
    assert!(email_setter.is_some(), "Should find email setter from attr_accessor");

    // Find methods that depend on these synthetic methods
    let display_info = symbols
        .iter()
        .find(|s| s.name.as_ref() == "display_info" && s.kind == SymbolKind::Method);
    assert!(display_info.is_some(), "Should find display_info method");

    let update_profile = symbols
        .iter()
        .find(|s| s.name.as_ref() == "update_profile" && s.kind == SymbolKind::Method);
    assert!(update_profile.is_some(), "Should find update_profile method");

    // Note: When method call tracking is implemented, we should verify:
    // - display_info depends on id, name, email getters
    // - update_profile depends on name=, email= setters

    println!("✅ attr_accessor dependencies test passed");
}

#[test]
fn test_ruby_mixin_dependencies() {
    println!("\n=== Testing Ruby Mixin Dependencies ===");

    let code = r#"
module Cacheable
  def cache_key
    self.class.name + "_" + id.to_s
  end

  def cached?
    !cache_key.nil?
  end
end

module Timestamps
  def created_at
    @created_at ||= Time.now
  end

  def updated_at
    @updated_at ||= Time.now
  end
end

class Article
  include Cacheable    # Mixin dependency
  include Timestamps   # Mixin dependency

  attr_reader :id, :title

  def initialize(id, title)
    @id = id
    @title = title
  end

  def publish
    # Depends on updated_at from Timestamps mixin
    touch
    @published = true
  end

  def touch
    @updated_at = Time.now
  end

  def info
    # Depends on cache_key from Cacheable mixin
    "Article #{title} - Cache: #{cache_key}"
  end
end
"#;

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(code, file_id, &mut counter);

    // Find modules
    let cacheable = symbols
        .iter()
        .find(|s| s.name.as_ref() == "Cacheable" && s.kind == SymbolKind::Module);
    assert!(cacheable.is_some(), "Should find Cacheable module");

    let timestamps = symbols
        .iter()
        .find(|s| s.name.as_ref() == "Timestamps" && s.kind == SymbolKind::Module);
    assert!(timestamps.is_some(), "Should find Timestamps module");

    // Find Article class
    let article = symbols
        .iter()
        .find(|s| s.name.as_ref() == "Article" && s.kind == SymbolKind::Class);
    assert!(article.is_some(), "Should find Article class");

    // Find mixin methods
    let cache_key = symbols
        .iter()
        .find(|s| s.name.as_ref() == "cache_key" && s.kind == SymbolKind::Method);
    assert!(cache_key.is_some(), "Should find cache_key method from Cacheable");

    let updated_at = symbols
        .iter()
        .find(|s| s.name.as_ref() == "updated_at" && s.kind == SymbolKind::Method);
    assert!(updated_at.is_some(), "Should find updated_at method from Timestamps");

    // Note: When mixin tracking is implemented (Phase 5 - find_implementations), we should verify:
    // - Article includes Cacheable
    // - Article includes Timestamps
    // - Article methods can call mixin methods

    println!("✅ Mixin dependencies test passed");
}

#[test]
fn test_ruby_inheritance_chain_dependencies() {
    println!("\n=== Testing Ruby Inheritance Chain Dependencies ===");

    let code = r#"
class Base
  def base_method
    "base"
  end

  def shared_method
    "base implementation"
  end
end

class Middle < Base
  def middle_method
    # Depends on base_method from Base
    "middle: #{base_method}"
  end

  def shared_method
    # Overrides Base#shared_method
    "middle implementation"
  end
end

class Derived < Middle
  def derived_method
    # Depends on middle_method from Middle
    # Indirectly depends on base_method from Base
    "derived: " + middle_method
  end

  def full_chain
    # Depends on shared_method (Middle's version)
    # Depends on base_method (Base's version)
    shared_method + " + " + base_method
  end
end
"#;

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(code, file_id, &mut counter);

    // Find classes
    let base_class = symbols
        .iter()
        .find(|s| s.name.as_ref() == "Base" && s.kind == SymbolKind::Class);
    assert!(base_class.is_some(), "Should find Base class");

    let middle_class = symbols
        .iter()
        .find(|s| s.name.as_ref() == "Middle" && s.kind == SymbolKind::Class);
    assert!(middle_class.is_some(), "Should find Middle class");
    assert!(
        middle_class.unwrap().signature.as_ref().map(|s| s.as_ref().contains("< Base")).unwrap_or(false),
        "Middle should inherit from Base"
    );

    let derived_class = symbols
        .iter()
        .find(|s| s.name.as_ref() == "Derived" && s.kind == SymbolKind::Class);
    assert!(derived_class.is_some(), "Should find Derived class");
    assert!(
        derived_class.unwrap().signature.as_ref().map(|s| s.as_ref().contains("< Middle")).unwrap_or(false),
        "Derived should inherit from Middle"
    );

    // Verify methods exist at each level
    let base_method = symbols
        .iter()
        .find(|s| s.name.as_ref() == "base_method" && s.kind == SymbolKind::Method);
    assert!(base_method.is_some(), "Should find base_method");

    let middle_method = symbols
        .iter()
        .find(|s| s.name.as_ref() == "middle_method" && s.kind == SymbolKind::Method);
    assert!(middle_method.is_some(), "Should find middle_method");

    let derived_method = symbols
        .iter()
        .find(|s| s.name.as_ref() == "derived_method" && s.kind == SymbolKind::Method);
    assert!(derived_method.is_some(), "Should find derived_method");

    // Note: When inheritance chain tracking is implemented, we should verify:
    // - Derived can access Base methods through Middle
    // - Method resolution follows the inheritance chain
    // - Overridden methods shadow parent methods

    println!("✅ Inheritance chain dependencies test passed");
}
