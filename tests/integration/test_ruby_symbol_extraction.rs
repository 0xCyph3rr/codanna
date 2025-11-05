use codanna::parsing::ruby::parser::RubyParser;
use codanna::types::SymbolCounter;
use codanna::{FileId, SymbolKind, Visibility};
use std::fs;

#[test]
fn test_ruby_class_extraction() {
    println!("\n=== Testing Ruby Class Symbol Extraction ===");

    let code = r#"
class User
  def initialize(name)
    @name = name
  end
end

class Admin < User
  def initialize(name, level)
    super(name)
    @level = level
  end
end
"#;

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(code, file_id, &mut counter);

    println!("Extracted {} symbols", symbols.len());
    for symbol in &symbols {
        println!(
            "  - {} ({:?}) at {}:{}-{}:{}",
            symbol.name.as_ref(),
            symbol.kind,
            symbol.range.start_line,
            symbol.range.start_column,
            symbol.range.end_line,
            symbol.range.end_column
        );
    }

    // Find User class
    let user_class = symbols
        .iter()
        .find(|s| s.name.as_ref() == "User" && s.kind == SymbolKind::Class);
    assert!(user_class.is_some(), "Should find User class");

    let user = user_class.unwrap();
    assert_eq!(user.kind, SymbolKind::Class);
    assert_eq!(
        user.signature.as_ref().map(|s| s.as_ref()),
        Some("class User")
    );
    assert_eq!(user.visibility, Visibility::Public);

    // Find Admin class with inheritance
    let admin_class = symbols
        .iter()
        .find(|s| s.name.as_ref() == "Admin" && s.kind == SymbolKind::Class);
    assert!(admin_class.is_some(), "Should find Admin class");

    let admin = admin_class.unwrap();
    assert_eq!(admin.kind, SymbolKind::Class);
    assert_eq!(
        admin.signature.as_ref().map(|s| s.as_ref()),
        Some("class Admin < User")
    );

    println!("✅ Class extraction test passed");
}

#[test]
fn test_ruby_module_extraction() {
    println!("\n=== Testing Ruby Module Symbol Extraction ===");

    let code = r#"
module Authentication
  VERSION = "1.0.0"

  def self.enabled?
    true
  end

  module OAuth
    PROVIDER = "github"

    def self.authenticate(token)
      validate_token(token)
    end
  end
end
"#;

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(code, file_id, &mut counter);

    println!("Extracted {} symbols", symbols.len());

    // Find Authentication module
    let auth_module = symbols
        .iter()
        .find(|s| s.name.as_ref() == "Authentication" && s.kind == SymbolKind::Module);
    assert!(auth_module.is_some(), "Should find Authentication module");

    let auth = auth_module.unwrap();
    assert_eq!(auth.kind, SymbolKind::Module);
    assert_eq!(
        auth.signature.as_ref().map(|s| s.as_ref()),
        Some("module Authentication")
    );

    // Find nested OAuth module
    let oauth_module = symbols
        .iter()
        .find(|s| s.name.as_ref() == "OAuth" && s.kind == SymbolKind::Module);
    assert!(oauth_module.is_some(), "Should find OAuth nested module");

    let oauth = oauth_module.unwrap();
    assert_eq!(oauth.kind, SymbolKind::Module);
    assert_eq!(
        oauth.signature.as_ref().map(|s| s.as_ref()),
        Some("module OAuth")
    );

    println!("✅ Module extraction test passed");
}

#[test]
fn test_ruby_method_extraction() {
    println!("\n=== Testing Ruby Method Symbol Extraction ===");

    let code = r#"
class User
  def initialize(name, email)
    @name = name
    @email = email
  end

  def self.find(id)
    new(id, "user#{id}@example.com")
  end

  def self.count
    @@count
  end

  def greet
    "Hello, #{@name}"
  end

  def update(attributes)
    @name = attributes[:name]
  end

  private

  def validate
    !@name.nil?
  end

  def normalize_email
    @email.downcase
  end

  protected

  def internal_id
    @name + "_" + @email
  end

  public

  def to_s
    "User(#{@name})"
  end
end
"#;

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(code, file_id, &mut counter);

    println!("Extracted {} symbols", symbols.len());
    for symbol in &symbols {
        if symbol.kind == SymbolKind::Method {
            println!(
                "  - Method: {} ({:?}) - signature: {:?}",
                symbol.name.as_ref(),
                symbol.visibility,
                symbol.signature.as_ref().map(|s| s.as_ref())
            );
        }
    }

    // Test instance methods
    let initialize = symbols
        .iter()
        .find(|s| s.name.as_ref() == "initialize" && s.kind == SymbolKind::Method);
    assert!(initialize.is_some(), "Should find initialize method");
    assert_eq!(
        initialize.unwrap().signature.as_ref().map(|s| s.as_ref()),
        Some("def initialize(name, email)")
    );

    let greet = symbols
        .iter()
        .find(|s| s.name.as_ref() == "greet" && s.kind == SymbolKind::Method);
    assert!(greet.is_some(), "Should find greet method");
    assert_eq!(greet.unwrap().visibility, Visibility::Public);

    // Test class methods
    let find = symbols
        .iter()
        .find(|s| s.name.as_ref() == "find" && s.kind == SymbolKind::Method);
    assert!(find.is_some(), "Should find class method find");
    assert_eq!(
        find.unwrap().signature.as_ref().map(|s| s.as_ref()),
        Some("def self.find(id)")
    );

    let count = symbols
        .iter()
        .find(|s| s.name.as_ref() == "count" && s.kind == SymbolKind::Method);
    assert!(count.is_some(), "Should find class method count");
    assert_eq!(
        count.unwrap().signature.as_ref().map(|s| s.as_ref()),
        Some("def self.count")
    );

    // Test visibility
    let validate = symbols
        .iter()
        .find(|s| s.name.as_ref() == "validate" && s.kind == SymbolKind::Method);
    assert!(validate.is_some(), "Should find private method validate");
    assert_eq!(validate.unwrap().visibility, Visibility::Private);

    let internal_id = symbols
        .iter()
        .find(|s| s.name.as_ref() == "internal_id" && s.kind == SymbolKind::Method);
    assert!(
        internal_id.is_some(),
        "Should find protected method internal_id"
    );
    assert_eq!(internal_id.unwrap().visibility, Visibility::Module);

    let to_s = symbols
        .iter()
        .find(|s| s.name.as_ref() == "to_s" && s.kind == SymbolKind::Method);
    assert!(to_s.is_some(), "Should find public method to_s");
    assert_eq!(to_s.unwrap().visibility, Visibility::Public);

    println!("✅ Method extraction test passed");
}

#[test]
fn test_ruby_constant_extraction() {
    println!("\n=== Testing Ruby Constant Symbol Extraction ===");

    let code = r#"
module Authentication
  VERSION = "1.0.0"
  DEFAULT_TIMEOUT = 30
end

class User
  MAX_LOGIN_ATTEMPTS = 3
  DEFAULT_ROLE = "guest"
  PERMISSIONS = ["read", "write", "delete"]
  CONFIG = { timeout: 30, retry: true }
end
"#;

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(code, file_id, &mut counter);

    println!("Extracted {} symbols", symbols.len());
    for symbol in &symbols {
        if symbol.kind == SymbolKind::Constant {
            println!(
                "  - Constant: {} - signature: {:?}",
                symbol.name.as_ref(),
                symbol.signature.as_ref().map(|s| s.as_ref())
            );
        }
    }

    // Test module constants
    let version = symbols
        .iter()
        .find(|s| s.name.as_ref() == "VERSION" && s.kind == SymbolKind::Constant);
    assert!(version.is_some(), "Should find VERSION constant");
    assert_eq!(
        version.unwrap().signature.as_ref().map(|s| s.as_ref()),
        Some("VERSION = \"1.0.0\"")
    );
    assert_eq!(version.unwrap().visibility, Visibility::Public);

    let timeout = symbols
        .iter()
        .find(|s| s.name.as_ref() == "DEFAULT_TIMEOUT" && s.kind == SymbolKind::Constant);
    assert!(timeout.is_some(), "Should find DEFAULT_TIMEOUT constant");
    assert_eq!(
        timeout.unwrap().signature.as_ref().map(|s| s.as_ref()),
        Some("DEFAULT_TIMEOUT = 30")
    );

    // Test class constants
    let max_attempts = symbols
        .iter()
        .find(|s| s.name.as_ref() == "MAX_LOGIN_ATTEMPTS" && s.kind == SymbolKind::Constant);
    assert!(
        max_attempts.is_some(),
        "Should find MAX_LOGIN_ATTEMPTS constant"
    );
    assert_eq!(
        max_attempts.unwrap().signature.as_ref().map(|s| s.as_ref()),
        Some("MAX_LOGIN_ATTEMPTS = 3")
    );

    let permissions = symbols
        .iter()
        .find(|s| s.name.as_ref() == "PERMISSIONS" && s.kind == SymbolKind::Constant);
    assert!(permissions.is_some(), "Should find PERMISSIONS constant");
    assert_eq!(
        permissions.unwrap().signature.as_ref().map(|s| s.as_ref()),
        Some("PERMISSIONS = [...]")
    );

    let config = symbols
        .iter()
        .find(|s| s.name.as_ref() == "CONFIG" && s.kind == SymbolKind::Constant);
    assert!(config.is_some(), "Should find CONFIG constant");
    assert_eq!(
        config.unwrap().signature.as_ref().map(|s| s.as_ref()),
        Some("CONFIG = {{...}}")
    );

    println!("✅ Constant extraction test passed");
}

#[test]
fn test_ruby_attr_accessor_extraction() {
    println!("\n=== Testing Ruby attr_accessor Symbol Extraction ===");

    let code = r#"
class User
  attr_reader :id, :username
  attr_accessor :email, :role
  attr_writer :password
end
"#;

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(code, file_id, &mut counter);

    println!("Extracted {} symbols", symbols.len());
    for symbol in &symbols {
        if symbol.kind == SymbolKind::Method {
            println!(
                "  - Method: {} - signature: {:?}",
                symbol.name.as_ref(),
                symbol.signature.as_ref().map(|s| s.as_ref())
            );
        }
    }

    // Test attr_reader (getters only)
    let id_getter = symbols
        .iter()
        .find(|s| s.name.as_ref() == "id" && s.kind == SymbolKind::Method);
    assert!(
        id_getter.is_some(),
        "Should find id getter from attr_reader"
    );
    assert_eq!(
        id_getter.unwrap().signature.as_ref().map(|s| s.as_ref()),
        Some("def id")
    );

    let id_setter = symbols.iter().find(|s| s.name.as_ref() == "id=");
    assert!(
        id_setter.is_none(),
        "Should NOT find id setter from attr_reader"
    );

    // Test attr_accessor (getters and setters)
    let email_getter = symbols
        .iter()
        .find(|s| s.name.as_ref() == "email" && s.kind == SymbolKind::Method);
    assert!(
        email_getter.is_some(),
        "Should find email getter from attr_accessor"
    );

    let email_setter = symbols
        .iter()
        .find(|s| s.name.as_ref() == "email=" && s.kind == SymbolKind::Method);
    assert!(
        email_setter.is_some(),
        "Should find email setter from attr_accessor"
    );
    assert_eq!(
        email_setter.unwrap().signature.as_ref().map(|s| s.as_ref()),
        Some("def email=(value)")
    );

    // Test attr_writer (setters only)
    let password_getter = symbols
        .iter()
        .find(|s| s.name.as_ref() == "password" && s.kind == SymbolKind::Method);
    assert!(
        password_getter.is_none(),
        "Should NOT find password getter from attr_writer"
    );

    let password_setter = symbols
        .iter()
        .find(|s| s.name.as_ref() == "password=" && s.kind == SymbolKind::Method);
    assert!(
        password_setter.is_some(),
        "Should find password setter from attr_writer"
    );

    println!("✅ attr_accessor extraction test passed");
}

#[test]
fn test_ruby_comprehensive_fixture() {
    println!("\n=== Testing Ruby Comprehensive Fixture ===");

    let fixture_path = "examples/ruby/comprehensive.rb";
    let code = fs::read_to_string(fixture_path).expect("Failed to read comprehensive Ruby fixture");

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(&code, file_id, &mut counter);

    println!(
        "Extracted {} symbols from comprehensive fixture",
        symbols.len()
    );

    // Count symbols by kind
    let mut class_count = 0;
    let mut module_count = 0;
    let mut method_count = 0;
    let mut constant_count = 0;

    for symbol in &symbols {
        match symbol.kind {
            SymbolKind::Class => class_count += 1,
            SymbolKind::Module => module_count += 1,
            SymbolKind::Method => method_count += 1,
            SymbolKind::Constant => constant_count += 1,
            _ => {}
        }
    }

    println!("Symbol counts:");
    println!("  - Classes: {}", class_count);
    println!("  - Modules: {}", module_count);
    println!("  - Methods: {}", method_count);
    println!("  - Constants: {}", constant_count);

    // Validate minimum symbol counts (based on comprehensive.rb content)
    assert!(class_count >= 10, "Should find at least 10 classes");
    assert!(module_count >= 5, "Should find at least 5 modules");
    assert!(method_count >= 30, "Should find at least 30 methods");
    assert!(constant_count >= 5, "Should find at least 5 constants");

    // Validate specific symbols from the comprehensive fixture
    assert!(
        symbols
            .iter()
            .any(|s| s.name.as_ref() == "User" && s.kind == SymbolKind::Class),
        "Should find User class"
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name.as_ref() == "Admin" && s.kind == SymbolKind::Class),
        "Should find Admin class"
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name.as_ref() == "Authentication" && s.kind == SymbolKind::Module),
        "Should find Authentication module"
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name.as_ref() == "OAuth" && s.kind == SymbolKind::Module),
        "Should find OAuth module"
    );

    println!("✅ Comprehensive fixture test passed");
}

#[test]
fn test_ruby_user_fixture() {
    println!("\n=== Testing Ruby User Fixture ===");

    let fixture_path = "examples/ruby/user.rb";
    let code = fs::read_to_string(fixture_path).expect("Failed to read user Ruby fixture");

    let mut parser = RubyParser::new().expect("Failed to create Ruby parser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).expect("Failed to create FileId");

    let symbols = parser.parse(&code, file_id, &mut counter);

    println!("Extracted {} symbols from user fixture", symbols.len());

    // Validate Models::User class
    assert!(
        symbols
            .iter()
            .any(|s| s.name.as_ref() == "User" && s.kind == SymbolKind::Class),
        "Should find User class"
    );

    // Validate methods
    assert!(
        symbols
            .iter()
            .any(|s| s.name.as_ref() == "initialize" && s.kind == SymbolKind::Method),
        "Should find initialize method"
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name.as_ref() == "find" && s.kind == SymbolKind::Method),
        "Should find find class method"
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name.as_ref() == "update" && s.kind == SymbolKind::Method),
        "Should find update method"
    );

    // Validate private methods
    let validate = symbols
        .iter()
        .find(|s| s.name.as_ref() == "validate!" && s.kind == SymbolKind::Method);
    assert!(validate.is_some(), "Should find private validate! method");
    assert_eq!(validate.unwrap().visibility, Visibility::Private);

    // Validate constants
    assert!(
        symbols
            .iter()
            .any(|s| s.name.as_ref() == "MAX_NAME_LENGTH" && s.kind == SymbolKind::Constant),
        "Should find MAX_NAME_LENGTH constant"
    );

    println!("✅ User fixture test passed");
}
