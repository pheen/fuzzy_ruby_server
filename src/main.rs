mod persistence;

use persistence::Persistence;
use tasklist::tasklist;

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
        persistence.initialize(&params);
        drop(persistence);

        tokio::spawn(async move {
            #[cfg(not(target_family = "windows"))]
            loop {
                let editor_process_id = params.process_id.unwrap_or_else(|| quit::with_code(1));

                let editor_process_running = psutil::process::processes()
                    .unwrap()
                    .into_iter()
                    .filter_map(|process| process.ok())
                    .find(|process| process.pid() == editor_process_id);

                if let None = editor_process_running {
                    quit::with_code(1);
                }

                tokio::time::sleep(Duration::from_secs(60)).await;
            }
            #[cfg(target_family = "Windows")]
            loop {
                use tasklist;

                let editor_process_id = params.process_id.unwrap_or_else(|| quit::with_code(1));

                let editor_process_running = unsafe {
                    let mut tl = tasklist::Tasklist::new();
                    tl.any(|process| process.get_pid() == editor_process_id)
                };

                if !editor_process_running {
                    quit::with_code(1);
                }

                tokio::time::sleep(Duration::from_secs(60)).await;
            }

        });

        let background_persistence = Arc::clone(&self.persistence);

        tokio::spawn(async move {
            loop {
                let mut persistence = background_persistence.lock().await;
                let _ = persistence.reindex_modified_files();
                let _ = persistence.index_included_dirs_once();
                let _ = persistence.index_gems_once();
                drop(persistence);

                tokio::time::sleep(Duration::from_secs(600)).await
            }
        });

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
                workspace_symbol_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let mut persistence = self.persistence.lock().await;
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

        if persistence.report_diagnostics {
            self.client
                .publish_diagnostics(
                    params.text_document.uri,
                    diagnostics,
                    Some(params.text_document.version),
                )
                .await;
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let mut persistence = self.persistence.lock().await;

        for content_change in &params.content_changes {
            persistence
                .reindex_modified_file(
                    &self.client,
                    &content_change.text,
                    &params.text_document.uri,
                )
                .await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let mut persistence = self.persistence.lock().await;
        persistence
            .reindex_modified_file(
                &self.client,
                &params.text.unwrap(),
                &params.text_document.uri,
            )
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

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let persistence = self.persistence.lock().await;

        let symbol_info_response = || -> Option<Vec<SymbolInformation>> {
            let documents = persistence
                .find_references_in_workspace(params.query)
                .unwrap_or_else(|_| Vec::new());
            let symbol_info = persistence.documents_to_symbol_information(documents);

            Some(symbol_info)
        }();

        Ok(symbol_info_response)
    }
}
