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

# Test execution
if __FILE__ == $PROGRAM_NAME
  puts "Running comprehensive Ruby parser test"
  example_method_calls
  puts "Test complete"
end
