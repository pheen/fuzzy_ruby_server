require "json"

module FuzzyRubyServer
  class Workspace
    def initialize(connection)
      @connection = connection
      @event_handler = EventHandler.new
    end

    def listen
      consecutive_parse_errors = 0

      loop do
        # stop listening, the client likely disconnected
        return if consecutive_parse_errors >= 5

        # wait for a message from the client
        headers = @connection.gets

        content_length = content_length(headers)
        message = read_message(content_length)

        if message
          consecutive_parse_errors = 0
          send_response(message)
        else
          consecutive_parse_errors += 1
        end
      rescue JSON::ParserError
        Log.error("JSON parse error: #{json}")
      rescue Exception => e
        Log.error("Something exploded: #{e}")
        Log.error("Backtrace:\n#{e.backtrace * "\n"}")
      end
    end

    private

    def content_length(headers)
      if headers.respond_to?(:match)
        content_length = headers.match(/Content-Length: (\d+)/)[1].to_i
      end
    end

    def read_message(content_length)
      return unless content_length

      # not used, part of the protocol
      _clrf = @connection.gets

      bytes_remaining = content_length.dup
      json = ""

      while bytes_remaining > 0
        json_chunk = @connection.readpartial(bytes_remaining)

        json += json_chunk
        bytes_remaining -= json_chunk.bytesize
      end

      log_json(json)

      JSON.parse(json.strip)
    end

    def send_response(message)
      method_name = message["method"].gsub("/", "_").freeze
      method_name = "on_#{method_name}".freeze

      return unless @event_handler.respond_to?(event_name)

      result = @event_handler.public_send(
        method_name,
        message["params"]
      )
      write_response(json, result)
    end

    def log_json(json)
      if json.bytesize < 2000
        Log.debug("Received json: #{json}")
      else
        Log.debug("Received large json blob")
      end
    end
  end
end
