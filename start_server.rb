require "pry"

Dir["#{File.expand_path(File.dirname(__FILE__))}/**/*.rb"].each do |file|
  require file
end

FuzzyRubyServer::Server.start
