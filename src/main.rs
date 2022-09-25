mod persistence;

use persistence::Persistence;

use tower_lsp::jsonrpc::{Response, Result};
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

use std::borrow::{Borrow, BorrowMut};
// use std::sync::{Arc, Mutex};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    env_logger::init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let persistence = Arc::new(Mutex::new(Persistence::new().unwrap()));
    let cloned_persistence = Arc::clone(&persistence);

    tokio::spawn(async move {
        loop {
            info!("Loop started.");

            let mut loop_persistence = cloned_persistence.lock().await;

            match loop_persistence.reindex_modified_files() {
                Ok(_) => {
                    drop(loop_persistence);
                    tokio::time::sleep(Duration::from_secs(30)).await
                },
                Err(_) => {}
            }

            info!("Loop ended.");
        };
    });

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

// use std::thread;
// use std::time::Duration;

use tokio::runtime::Runtime;
use tokio::time::*;

use std::cell::RefCell;
use std::rc::Rc;

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let mut persistence = self.persistence.lock().await;
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
                // text_document_sync: Some(TextDocumentSyncCapability::Kind(
                //     TextDocumentSyncKind::FULL,
                // )),
                definition_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        // self.client
        //     .log_message(MessageType::INFO, "server initialized!")
        //     .await;

        // let persistence = self.persistence.lock().await;
        // persistence.reindex_modified_files_loop();

        // self.persistence.reindex_modified_files();

        // let self_clone: Arc<&Backend> = Arc::clone(self);

        // tokio::spawn(async move {
        //     loop {
        //         info!("Loop started!");

        //         let persistence = self_clone.persistence.lock().await;

        //         persistence.reindex_modified_files();

        //         info!("Loop ended, sleeping...");
        //         tokio::time::sleep(Duration::from_secs(100000));
        //     };
        // });


        // let mut rt = Runtime::new().unwrap();

        // rt.block_on(async move {
        //     println!("hello from the async block");
        //     async_function("task0").await;

        //     //bonus, you could spawn tasks too
        //     tokio::spawn(async { async_function("task1").await });
        //     tokio::spawn(async { async_function("task2").await });
        // });

        // loop {}

    // let new_self = Arc::new(Mutex::new(&self));

    // tokio::spawn(async {
    //     loop {
    //         info!("Loop started!");

    //         let new_new_self = new_self.lock().await;
    //         let persistence =  new_new_self.persistence.lock().await;

    //         persistence.reindex_modified_files();

    //         info!("Loop ended, sleeping...");
    //         tokio::time::sleep(Duration::from_secs(10));
    //     };
    // });


        // thread::spawn(|| {


        //     let mut rt = Runtime::new().unwrap();
        //     rt.block_on(async move {
        //         println!("hello from the async block");
        //         async_function("task0").await;

        //         //bonus, you could spawn tasks too
        //         tokio::spawn(async { async_function("task1").await });
        //         tokio::spawn(async { async_function("task2").await });
        //     });
        //     loop {}
        // });
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let persistence = self.persistence.lock().await;
        let mut diagnostics: Vec<tower_lsp::lsp_types::Diagnostic> = vec![];

        let change_diagnostics =
            // persistence.reindex_modified_file(&params.text_document.text, &params.text_document.uri);
            persistence.diagnostics(&params.text_document.text, &params.text_document.uri);

        for diagnostic in change_diagnostics {
            for unwrapped_diagnostic in diagnostic {
                if let Some(finally_diagnostic) = unwrapped_diagnostic {
                    &diagnostics.push(finally_diagnostic.to_owned());
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
                        &diagnostics.push(finally_diagnostic.to_owned());
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
                    &diagnostics.push(finally_diagnostic.to_owned());
                }
            }
        }

        self.client
            .publish_diagnostics(
                params.text_document.uri,
                diagnostics,
                None,
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
}
