# User model for testing cross-module resolution
module Models
  class User
    attr_reader :id, :name, :email
    attr_accessor :role

    MAX_NAME_LENGTH = 50
    DEFAULT_ROLE = "user"

    def initialize(id, name, email)
      @id = id
      @name = name
      @email = email
      @role = DEFAULT_ROLE
    end

    def self.find(id)
      # Simulate database lookup
      new(id, "User #{id}", "user#{id}@example.com")
    end

    def self.create(attributes)
      new(
        attributes[:id],
        attributes[:name],
        attributes[:email]
      )
    end

    def update(attributes)
      @name = attributes[:name] if attributes[:name]
      @email = attributes[:email] if attributes[:email]
      validate!
      true
    end

    def valid?
      !@name.nil? && !@email.nil? && @name.length <= MAX_NAME_LENGTH
    end

    def to_s
      "User(id=#{@id}, name=#{@name}, email=#{@email})"
    end

    private

    def validate!
      raise "Invalid user" unless valid?
    end

    def normalize_email
      @email.downcase.strip
    end

    protected

    def internal_id
      "user_#{@id}"
    end
  end
end
