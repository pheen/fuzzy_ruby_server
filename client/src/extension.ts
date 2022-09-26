/* --------------------------------------------------------------------------------------------
 * Copyright (c) Microsoft Corporation. All rights reserved.
 * Licensed under the MIT License. See License.txt in the project root for license information.
 * ------------------------------------------------------------------------------------------ */

import * as path from "path";
import {
  languages,
  workspace,
  EventEmitter,
  ExtensionContext,
  window,
  commands,
  ViewColumn,
  WebviewPanel,
  WorkspaceEdit,
  Selection,
  Uri,
  InlayHintsProvider,
  TextDocument,
  CancellationToken,
  Range,
  InlayHint,
  TextDocumentChangeEvent,
  Position,
  InlayHintLabelPart,
  Location,
  ProviderResult,
} from "vscode";

import {
  Disposable,
  Executable,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient;

export async function activate(context: ExtensionContext) {
  let base_dir = __dirname.split("/").slice(0, -2).join("/");

  const command = `${base_dir}/bin/fuzzy`;
  const run: Executable = {
    command,
    options: {
      env: {
        ...process.env,
        RUST_LOG: "debug",
        RUST_BACKTRACE: "1",
      },
    },
  };
  const serverOptions: ServerOptions = {
    run,
    debug: run,
  };
  // If the extension is launched in debug mode then the debug server options are used
  // Otherwise the run options are used
  // Options to control the language client
  let clientOptions: LanguageClientOptions = {
    // Register the server for plain text documents
    documentSelector: [{ scheme: "file", language: "ruby" }],
    synchronize: {
      // Notify the server about file changes to '.clientrc files contained in the workspace
      // fileEvents: workspace.createFileSystemWatcher("**/.clientrc"),
    },
    // traceOutputChannel,
  };

  // Create the language client and start the client.
  client = new LanguageClient("fuzzy-ruby-language-server", "Fuzzy Ruby Language Server", serverOptions, clientOptions);
  // activateInlayHints(context);
  client.start();
}

export function deactivate(): Thenable<void> {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

export function activateInlayHints(ctx: ExtensionContext) {
  const maybeUpdater = {
    hintsProvider: null as Disposable | null,
    updateHintsEventEmitter: new EventEmitter<void>(),

    async onConfigChange() {
      this.dispose();

      const event = this.updateHintsEventEmitter.event;
      this.hintsProvider = languages.registerInlayHintsProvider(
        { scheme: "file", language: "ruby" },
        new (class implements InlayHintsProvider {
          onDidChangeInlayHints = event;
          resolveInlayHint(hint: InlayHint, token: CancellationToken): ProviderResult<InlayHint> {
            return {
              label: hint.label,
              ...hint
            };
          }
          async provideInlayHints(
            document: TextDocument,
            range: Range,
            token: CancellationToken
          ): Promise<InlayHint[]> {
            const hints = (await client
              .sendRequest("custom/inlay_hint", { path: document.uri.toString() })
              .catch(err => null)) as [number, number, string][];
            if (hints == null) {
              return [];
            } else {
              return hints.map(item => {
                const [start, end, label] = item;
                let startPosition = document.positionAt(start);
                let endPosition = document.positionAt(end);
                return {
                  position: endPosition,
                  paddingLeft: true,
                  label: [
                    {
                      value: label,
                      location: new Location(document.uri, startPosition),
                    },
                  ],
                };
              });
            }
          }
        })()
      );
    },

    onDidChangeTextDocument({ contentChanges, document }: TextDocumentChangeEvent) {
      // debugger
      // this.updateHintsEventEmitter.fire();
    },

    dispose() {
      this.hintsProvider?.dispose();
      this.hintsProvider = null;
      this.updateHintsEventEmitter.dispose();
    },
  };

  workspace.onDidChangeConfiguration(maybeUpdater.onConfigChange, maybeUpdater, ctx.subscriptions);
  workspace.onDidChangeTextDocument(maybeUpdater.onDidChangeTextDocument, maybeUpdater, ctx.subscriptions);

  maybeUpdater.onConfigChange().catch(console.error);
}
