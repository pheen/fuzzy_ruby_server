import {
  ExtensionContext,
  workspace,
} from "vscode";

import {
  Executable,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient;

export async function activate(_context: ExtensionContext) {
  let base_dir = __dirname.split("/").slice(0, -2).join("/");
  let command = undefined;

  if (process.platform == "darwin") {
    if (process.arch == "arm64") {
      // Apple
      command = `${base_dir}/bin/fuzzy_darwin-arm64`;
    } else {
    // Apple/Intel
      command = `${base_dir}/bin/fuzzy`;
    }
  } else if ("windows") {
    command = `${base_dir}/bin/fuzzy.exe`;
  } else {
    command = `${base_dir}/bin/fuzzy_x86_64-unknown-linux-gnu`;
  }

  // command = "/Users/joelkorpela/dev/fuzzy_ruby_server/target/aarch64-apple-darwin/release/fuzzy";

  const run: Executable = {
    command,
    options: {
      env: {
        ...process.env,
        RUST_LOG: "info",
        RUST_BACKTRACE: "1",
      },
    },
  };

  // If the extension is launched in debug mode then the debug server options
  // are used Otherwise the run options are used
  const serverOptions: ServerOptions = {
    run,
    debug: run,
  };

  const client_config = workspace.getConfiguration("fuzzyRubyServer");

  let clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "ruby" }
    ],
    synchronize: {
      // fileEvents: workspace.createFileSystemWatcher("**/.clientrc"),
    },
    initializationOptions: {
      allocationType: client_config.get("allocationType"),
      indexGems: client_config.get("indexGems"),
      reportDiagnostics: client_config.get("reportDiagnostics"),
    },
  };

  // Create the language client and start the client.
  client = new LanguageClient("fuzzy-ruby-server", "Fuzzy Ruby Server", serverOptions, clientOptions);
  client.start();
}

export function deactivate(): Thenable<void> {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
