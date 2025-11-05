# Comprehensive Ruby test file for codanna parser validation
# Covers all major Ruby language features

# Module definition with constants
module Authentication
  VERSION = "1.0.0"
  DEFAULT_TIMEOUT = 30

  # Module method
  def self.enabled?
    true
  end

  # Nested module
  module OAuth
    PROVIDER = "github"

    def self.authenticate(token)
      validate_token(token)
    end

    def self.validate_token(token)
      !token.nil? && !token.empty?
    end
  end
end

# Class definition with inheritance
class User
  # Class variables
  @@user_count = 0

  # Constants
  MAX_LOGIN_ATTEMPTS = 3
  DEFAULT_ROLE = "guest"

  # Instance variables with attr accessors
  attr_reader :id, :username
  attr_accessor :email, :role
  attr_writer :password

  # Class methods
  def self.find(id)
    new(id, "user_#{id}")
  end

  def self.count
    @@user_count
  end

  # Constructor
  def initialize(id, username, email: nil, role: DEFAULT_ROLE)
    @id = id
    @username = username
    @email = email
    @role = role
    @login_attempts = 0
    @@user_count += 1
  end

  # Instance methods
  def authenticate(password)
    return false if @login_attempts >= MAX_LOGIN_ATTEMPTS

    if valid_password?(password)
      reset_login_attempts
      true
    else
      @login_attempts += 1
      false
    end
  end

  def admin?
    @role == "admin"
  end

  def to_s
    "User(#{@id}, #{@username})"
  end

  private

  def valid_password?(password)
    !password.nil? && password.length >= 8
  end

  def reset_login_attempts
    @login_attempts = 0
  end

  protected

  def internal_id
    "#{@id}_#{@username}"
  end
end

# Class with module inclusion
class Admin < User
  include Authentication::OAuth
  extend Authentication

  PERMISSIONS = ["read", "write", "delete"]

  def initialize(id, username, email: nil)
    super(id, username, email: email, role: "admin")
    @permissions = PERMISSIONS.dup
  end

  def grant_permission(permission)
    @permissions << permission unless @permissions.include?(permission)
  end

  def has_permission?(permission)
    @permissions.include?(permission)
  end

  # Alternative class method syntax (ClassName.method_name)
  def Admin.from_user(user)
    new(user.id, user.username, email: user.email)
  end

  # Singleton class syntax (class << self)
  class << self
    def all_permissions
      PERMISSIONS
    end

    def validate_permission(permission)
      PERMISSIONS.include?(permission)
    end
  end
end

# Singleton class example
class Configuration
  @instance = nil

  def self.instance
    @instance ||= new
  end

  def initialize
    @settings = {}
  end

  def set(key, value)
    @settings[key] = value
  end

  def get(key)
    @settings[key]
  end

  private_class_method :new
end

# Module with mixins
module Cacheable
  def cache_key
    "#{self.class.name.downcase}_#{id}"
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

  def touch
    @updated_at = Time.now
  end
end

# Class with multiple mixins
class Article
  include Cacheable
  include Timestamps

  attr_reader :id, :title, :content

  def initialize(id, title, content)
    @id = id
    @title = title
    @content = content
  end

  def publish
    touch
    @published = true
  end

  def published?
    @published == true
  end
end

# Blocks, procs, and lambdas
class DataProcessor
  def self.process(items, &block)
    items.map(&block)
  end

  def self.filter(items)
    items.select { |item| yield(item) }
  end

  def self.with_logging
    puts "Starting operation"
    result = yield
    puts "Operation complete"
    result
  end
end

# Method calls and chaining
def example_method_calls
  user = User.find(1)
  user.email = "test@example.com"
  user.authenticate("password123")

  admin = Admin.new(2, "admin_user", email: "admin@example.com")
  admin.grant_permission("deploy")
  admin.has_permission?("deploy")

  # Block usage
  numbers = [1, 2, 3, 4, 5]
  doubled = DataProcessor.process(numbers) { |n| n * 2 }
  evens = DataProcessor.filter(numbers) { |n| n.even? }

  # Lambda
  multiply = ->(x, y) { x * y }
  result = multiply.call(3, 4)

  # Proc
  greeter = Proc.new { |name| "Hello, #{name}!" }
  greeting = greeter.call("World")

  # Method chaining
  article = Article.new(1, "Test", "Content")
  article.publish
  article.cached?

  config = Configuration.instance
  config.set(:debug, true)
  config.get(:debug)
end

# Metaprogramming examples
class DynamicModel
  def self.define_attribute(name)
    define_method(name) do
      instance_variable_get("@#{name}")
    end

    define_method("#{name}=") do |value|
      instance_variable_set("@#{name}", value)
    end
  end

  define_attribute :name
  define_attribute :age

  # method_missing for dynamic method handling
  def method_missing(method_name, *args, &block)
    if method_name.to_s.start_with?("dynamic_")
      "Handled dynamically: #{method_name}"
    else
      super
    end
  end

  def respond_to_missing?(method_name, include_private = false)
    method_name.to_s.start_with?("dynamic_") || super
  end
end

# Require statements (for dependency tracking)
require 'json'
require 'net/http'
require_relative 'authentication'

# Global variables (edge case)
$global_counter = 0

def increment_global
  $global_counter += 1
end

# Class with singleton methods
class Report
  def initialize(title)
    @title = title
  end

  def generate
    "Report: #{@title}"
  end
end

report = Report.new("Monthly Sales")

def report.custom_method
  "Custom behavior"
end

# Module prepending
module Auditable
  def save
    log_audit
    super
  end

  def log_audit
    puts "Audit: saving #{self.class.name}"
  end
end

class AuditedUser < User
  prepend Auditable

  def save
    # Save logic
    true
  end
end

# Nested class example
class OuterClass
  OUTER_CONSTANT = "outer"

  def outer_method
    "from outer"
  end

  # Nested inner class
  class InnerClass
    INNER_CONSTANT = "inner"

    def initialize
      @inner_var = "inner value"
    end

    def inner_method
      "from inner: #{@inner_var}"
    end

    # Access outer class constant
    def access_outer
      OuterClass::OUTER_CONSTANT
    end
  end

  # Another nested class
  class AnotherInner
    def another_method
      "another inner class"
    end
  end
end

# Edge cases for parser testing
class EdgeCases
  # Empty method
  def empty_method
  end

  # Method with splat operator
  def variable_args(*args)
    args.size
  end

  # Method with keyword arguments
  def keyword_args(required:, optional: "default")
    [required, optional]
  end

  # Method with block parameter
  def with_block(&block)
    block.call if block_given?
  end

  # Operator overloading
  def +(other)
    self.class.new
  end

  # Question mark method
  def valid?
    true
  end

  # Exclamation mark method
  def save!
    save || raise("Save failed")
  end

  # Method with multiple return values
  def stats
    [1, 2, 3]
  end
end

# ============================================================================
# COMPREHENSIVE MIXIN TEST CASES (Issue #13)
# Testing include, extend, and prepend with various patterns
# Expected AST: call nodes with method names "include"/"extend"/"prepend"
# ============================================================================

# Additional modules for mixin testing
module Loggable
  def log(message)
    puts "[LOG] #{message}"
  end
end

module Serializable
  def to_json
    "{ json representation }"
  end

  def from_json(data)
    "parsed: #{data}"
  end
end

module Validatable
  def validate
    true
  end

  def validate!
    validate || raise("Validation failed")
  end
end

# Nested modules for qualified name testing
module Features
  module Security
    def secure_hash(data)
      "hashed: #{data}"
    end
  end

  module Performance
    def benchmark
      start = Time.now
      yield if block_given?
      Time.now - start
    end
  end
end

# Multiple mixins in single statement - Class context
class MultiMixinClass
  # AST: call node with multiple arguments
  include Loggable, Serializable, Validatable

  def process
    validate && log("processing") && to_json
  end
end

# Multiple mixins with extend
class ExtendMultiple
  # AST: call node with multiple arguments (class methods)
  extend Loggable, Serializable

  def self.info
    log("class info")
    to_json
  end
end

# Multiple prepend (prepend takes precedence over include)
module TrackingA
  def save
    puts "TrackingA: before save"
    super
  end
end

module TrackingB
  def save
    puts "TrackingB: before save"
    super
  end
end

class MultiPrependClass
  # AST: call with multiple arguments (precedence: TrackingB > TrackingA > original)
  prepend TrackingB, TrackingA

  def save
    puts "Original save"
    true
  end
end

# Qualified module names (Module::Submodule syntax)
class QualifiedInclude
  # AST: call with scope_resolution in argument
  include Features::Security
  extend Features::Performance

  def secure_operation(data)
    secure_hash(data)
  end

  def self.timed_operation
    benchmark { sleep(0.1) }
  end
end

# Mixed qualified and simple names in one statement
class MixedQualified
  # AST: call with mixed argument types (simple + qualified)
  include Loggable, Features::Security, Serializable

  def secure_log(data)
    log(secure_hash(data))
  end
end

# Mixins in module context (not just classes)
module ServiceModule
  # AST: call within module body
  include Loggable
  extend Serializable

  def service_action
    log("service executing")
  end

  def self.describe
    to_json
  end
end

# Prepend in module context
module ChainableModule
  prepend Validatable

  def execute
    validate && perform_action
  end

  def perform_action
    "action performed"
  end
end

# Nested class with mixins
module Application
  class ServiceClass
    # AST: call within nested class
    include Loggable, Validatable
    extend Features::Performance

    def validated_action
      validate && log("action")
    end

    def self.benchmark_action
      benchmark { new.validated_action }
    end
  end

  module Helpers
    class UtilityClass
      # AST: multiple call nodes within deeply nested class
      prepend TrackingA
      include Serializable
      extend Features::Security

      def save
        to_json
      end

      def self.secure(data)
        secure_hash(data)
      end
    end
  end
end

# Singleton class with mixins
class SingletonWithMixins
  class << self
    # AST: call within singleton_class body
    include Loggable
    extend Serializable

    def singleton_log
      log("from singleton")
    end
  end
end

# All three mixin types in one class
class ComprehensiveMixins
  # AST: multiple call nodes with different method names
  prepend TrackingA              # Highest precedence
  include Loggable, Validatable  # Middle precedence
  extend Serializable            # Class methods

  def workflow
    validate && log("workflow") && save
  end

  def save
    "saved"
  end

  def self.export
    to_json
  end
end

# Conditional mixins (edge case - valid Ruby)
class ConditionalMixin
  # AST: call within if body
  if ENV['ENABLE_LOGGING']
    include Loggable
  end

  # AST: call within unless body
  unless ENV['DISABLE_VALIDATION']
    include Validatable
  end
end

# Mixin with inline module (advanced case)
class InlineMixin
  # AST: call with Module.new block argument
  include Module.new {
    def inline_method
      "from inline module"
    end

    def another_inline
      "also inline"
    end
  }
end

# Test execution
if __FILE__ == $PROGRAM_NAME
  puts "Running comprehensive Ruby parser test"
  example_method_calls
  puts "Test complete"
end
