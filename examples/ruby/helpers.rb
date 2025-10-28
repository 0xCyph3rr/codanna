# Helper utilities for testing cross-module calls
module Helpers
  VERSION = "1.0.0"
  LOG_PREFIX = "[HELPER]"

  def self.format_audit(id, name, permissions)
    timestamp = Time.now.strftime("%Y-%m-%d %H:%M:%S")
    "#{timestamp} | User #{id} (#{name}) | Permissions: #{permissions.join(', ')}"
  end

  def self.log_message(message)
    "#{LOG_PREFIX} #{Time.now.strftime("%H:%M:%S")} - #{message}"
  end

  def self.validate_email(email)
    email =~ /\A[\w+\-.]+@[a-z\d\-]+(\.[a-z\d\-]+)*\.[a-z]+\z/i
  end

  def self.sanitize_input(input)
    input.to_s.strip.gsub(/[<>]/, '')
  end

  def self.generate_token(length = 32)
    chars = ('a'..'z').to_a + ('A'..'Z').to_a + ('0'..'9').to_a
    Array.new(length) { chars.sample }.join
  end

  # Instance methods for mixin usage
  def format_timestamp
    Time.now.strftime("%Y-%m-%d %H:%M:%S")
  end

  def truncate_string(str, length = 50)
    str.length > length ? "#{str[0...length]}..." : str
  end
end

# Utility class for data processing
module Helpers
  class DataProcessor
    MAX_BATCH_SIZE = 100

    def initialize(batch_size = 10)
      @batch_size = [batch_size, MAX_BATCH_SIZE].min
      @processed = 0
    end

    def self.process_batch(items)
      processor = new(items.size)
      processor.process(items)
    end

    def process(items)
      items.each_slice(@batch_size) do |batch|
        process_single_batch(batch)
      end
      @processed
    end

    def stats
      {
        processed: @processed,
        batch_size: @batch_size
      }
    end

    private

    def process_single_batch(batch)
      batch.each { |item| process_item(item) }
      @processed += batch.size
    end

    def process_item(item)
      # Process individual item
      item
    end
  end
end
