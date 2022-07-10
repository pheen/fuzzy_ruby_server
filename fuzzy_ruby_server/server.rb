require "socket"

module FuzzyRubyServer
  class Server
    DefaultPort = 8937

    def self.start
      puts "Starting server..."

      socket = ::TCPServer.new(DefaultPort)

      loop do
        connection = socket.accept

        puts "connection accepted"

        Thread.new do
          Workspace.new(connection).listen
        end
      end
    rescue SignalException => e
      Log.error("Received kill signal: #{e}")
      exit(true)
    end
  end
end
