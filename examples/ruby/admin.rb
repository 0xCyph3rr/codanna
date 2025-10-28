# Admin model extending User
require_relative 'user'
require_relative 'helpers'

module Models
  class Admin < User
    PERMISSIONS = ["read", "write", "delete", "admin"]
    MAX_PERMISSIONS = 10

    def initialize(id, name, email)
      super(id, name, email)
      @role = "admin"
      @permissions = ["read", "write"]
    end

    def self.find(id)
      admin = super(id)
      admin.upgrade_to_admin
      admin
    end

    def self.create_super_admin(id, name, email)
      admin = new(id, name, email)
      admin.grant_all_permissions
      admin
    end

    def grant_permission(permission)
      return false if @permissions.length >= MAX_PERMISSIONS
      return false unless PERMISSIONS.include?(permission)

      @permissions << permission unless @permissions.include?(permission)
      log_permission_change("granted", permission)
      true
    end

    def revoke_permission(permission)
      if @permissions.delete(permission)
        log_permission_change("revoked", permission)
        true
      else
        false
      end
    end

    def has_permission?(permission)
      @permissions.include?(permission)
    end

    def grant_all_permissions
      @permissions = PERMISSIONS.dup
    end

    def can_delete?
      has_permission?("delete")
    end

    def can_administrate?
      has_permission?("admin")
    end

    # Uses helper method from helpers.rb
    def audit_log
      Helpers.format_audit(@id, @name, @permissions)
    end

    def to_s
      "Admin(id=#{@id}, name=#{@name}, permissions=#{@permissions.join(', ')})"
    end

    private

    def upgrade_to_admin
      @role = "admin"
      grant_permission("admin")
    end

    def log_permission_change(action, permission)
      # Uses helper from helpers.rb
      message = Helpers.log_message("Permission #{action}: #{permission} for #{@name}")
      puts message
    end

    protected

    def internal_id
      "admin_#{@id}"
    end
  end
end
