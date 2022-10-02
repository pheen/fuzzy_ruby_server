mod persistence;

use persistence::Persistence;

use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::*;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct Backend {
    client: Client,
    persistence: Arc<Mutex<Persistence>>,
}

#[tokio::main]
#[quit::main]
async fn main() {
    env_logger::init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let persistence = Arc::new(Mutex::new(Persistence::new().unwrap()));
    let cloned_persistence = Arc::clone(&persistence);

    tokio::spawn(async move {
        loop {
            let mut loop_persistence = cloned_persistence.lock().await;

            if !loop_persistence.editor_process_running() {
                quit::with_code(1);
            }

            if loop_persistence.no_workspace_confirmed() {
                quit::with_code(1);
            }

            match loop_persistence.reindex_modified_files() {
                Ok(_) => {
                    drop(loop_persistence);
                    tokio::time::sleep(Duration::from_secs(30)).await
                }
                Err(_) => {}
            }
        }
    });

    let (service, socket) = LspService::new(|client| Backend {
        client,
        persistence,
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let mut persistence = self.persistence.lock().await;

        persistence.set_process_id(params.process_id);
        persistence.set_workspace_path(params.root_uri);

        Ok(InitializeResult {
            server_info: None,
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL), // todo: incremental
                        will_save: Some(false),
                        will_save_wait_until: Some(false),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(true),
                        })),
                    },
                )),
                definition_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let persistence = self.persistence.lock().await;
        let mut diagnostics: Vec<tower_lsp::lsp_types::Diagnostic> = vec![];

        let change_diagnostics =
            persistence.diagnostics(&params.text_document.text, &params.text_document.uri);

        for diagnostic in change_diagnostics {
            for unwrapped_diagnostic in diagnostic {
                if let Some(finally_diagnostic) = unwrapped_diagnostic {
                    diagnostics.push(finally_diagnostic.to_owned());
                }
            }
        }

        self.client
            .publish_diagnostics(
                params.text_document.uri,
                diagnostics,
                Some(params.text_document.version),
            )
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let persistence = self.persistence.lock().await;
        let mut diagnostics: Vec<tower_lsp::lsp_types::Diagnostic> = vec![];

        for content_change in &params.content_changes {
            let change_diagnostics =
                &persistence.reindex_modified_file(&content_change.text, &params.text_document.uri);

            for diagnostic in change_diagnostics {
                for unwrapped_diagnostic in diagnostic {
                    if let Some(finally_diagnostic) = unwrapped_diagnostic {
                        diagnostics.push(finally_diagnostic.to_owned());
                    }
                }
            }
        }

        self.client
            .publish_diagnostics(
                params.text_document.uri,
                diagnostics,
                Some(params.text_document.version),
            )
            .await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let persistence = self.persistence.lock().await;
        let mut diagnostics: Vec<tower_lsp::lsp_types::Diagnostic> = vec![];
        let change_diagnostics =
            persistence.reindex_modified_file(&params.text.unwrap(), &params.text_document.uri);

        for diagnostic in change_diagnostics {
            for unwrapped_diagnostic in diagnostic {
                if let Some(finally_diagnostic) = unwrapped_diagnostic {
                    diagnostics.push(finally_diagnostic.to_owned());
                }
            }
        }

        self.client
            .publish_diagnostics(params.text_document.uri, diagnostics, None)
            .await;
    }

    async fn did_close(&self, _: DidCloseTextDocumentParams) {
        self.client
            .log_message(MessageType::INFO, "file closed!")
            .await;
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let persistence = self.persistence.lock().await;
        let definitions = || -> Option<GotoDefinitionResponse> {
            let locations = persistence.find_definitions(params.text_document_position_params);
            let locations = locations.unwrap();

            Some(GotoDefinitionResponse::Array(locations))
        }();

        Ok(definitions)
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let persistence = self.persistence.lock().await;

        let highlights_response = || -> Option<Vec<DocumentHighlight>> {
            let highlights = persistence.find_highlights(params.text_document_position_params);
            let highlights = highlights.unwrap();

            Some(highlights)
        }();

        Ok(highlights_response)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let persistence = self.persistence.lock().await;
        let text_position = params.clone().text_document_position;
        let text_document = &params.text_document_position.text_document;

        let locations_response = || -> Option<Vec<Location>> {
            let documents = persistence.find_references(text_position).unwrap();
            let locations = persistence.documents_to_locations(text_document.uri.path(), documents);

            Some(locations)
        }();

        Ok(locations_response)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let persistence = self.persistence.lock().await;
        let text_position = params.clone().text_document_position;
        let text_document = &params.text_document_position.text_document;
        let new_name = &params.new_name;

        let workspace_edit = || -> Option<WorkspaceEdit> {
            let references = persistence.find_references(text_position).unwrap();
            let workspace_edit =
                persistence.rename_tokens(text_document.uri.path(), references, new_name);

            Some(workspace_edit)
        }();

        Ok(workspace_edit)
    }
}
