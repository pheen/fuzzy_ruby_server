mod persistence;

use persistence::Persistence;

use tower_lsp::jsonrpc::{Result, Response};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use log::info;

// use std::fs::DirEntry;

// use lib_ruby_parser::source::DecodedInput;
// // use lib_ruby_parser::traverse::finder::PatternError;
// // ---
// // Importing tantivy...
// use tantivy::collector::TopDocs;
// use tantivy::query::QueryParser;
// use tantivy::schema::{self, *};
// use tantivy::{doc, Index, ReloadPolicy};
// // use tempfile::TempDir;

// use std::error::Error;
// use std::fs::{self, read_to_string};

// use filetime::FileTime;
// use lib_ruby_parser::{nodes::*, Node, Parser, ParserOptions};
// use walkdir::WalkDir;

use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    env_logger::init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    // let persistence = Persistence::new().unwrap();
    let persistence = Arc::new(Mutex::new(Persistence::new().unwrap()));

    let (service, socket) = LspService::new(|client| Backend {
        client,
        persistence,
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}

struct Backend {
    client: Client,
    // persistence: Persistence,
    // persistence: std::rc::Rc<std::cell::RefCell<Persistence>>,
    persistence: Arc<Mutex<Persistence>>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        self.persistence.lock().unwrap().set_workspace_path(params.root_uri);
        // self.persistence.lock()

        Ok(InitializeResult {
            server_info: None,
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                definition_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "server initialized!")
            .await;

        // self.persistence.lock().unwrap().reindex_modified_files();
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let persistence = self.persistence.lock().unwrap();
        persistence.reindex_modified_file(params.text_document);
    }

    async fn did_change(&self, _: DidChangeTextDocumentParams) {
        self.client
            .log_message(MessageType::INFO, "file changed!")
            .await;
    }

    async fn did_save(&self, _: DidSaveTextDocumentParams) {
        self.client
            .log_message(MessageType::INFO, "file saved!")
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
        let definitions = || -> Option<GotoDefinitionResponse> {
            let locations = self.persistence.lock().unwrap().find_definitions(params.text_document_position_params);
            let locations = locations.unwrap();

            Some(GotoDefinitionResponse::Array(locations))
        }();

        Ok(definitions)
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let highlights_response = || -> Option<Vec<DocumentHighlight>> {
            let highlights = self.persistence.lock().unwrap().find_highlights(params.text_document_position_params);
            let highlights = highlights.unwrap();

            Some(highlights)
        }();

        Ok(highlights_response)
    }
}
