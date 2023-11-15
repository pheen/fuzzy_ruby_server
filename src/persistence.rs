use filetime::FileTime;
use jwalk::WalkDirGeneric;
use lib_ruby_parser::source::DecodedInput;
use lib_ruby_parser::{nodes::*, Loc, Node, Parser, ParserOptions};
use log::info;
use phf::phf_map;
use regex::Regex;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::process::Command;
use std::str;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, Occur, Query, RegexQuery, TermQuery};
use tantivy::{schema::*, ReloadPolicy, Document};
use tantivy::{Index, IndexWriter};
use tower_lsp::lsp_types::InitializeParams;
use tower_lsp::lsp_types::{
    DocumentHighlight, DocumentHighlightKind, Location, Position, Range, SymbolInformation,
    SymbolKind, TextDocumentPositionParams, TextEdit, Url, WorkspaceEdit,
};
use tower_lsp::Client;

static USAGE_TYPE_RESTRICTIONS: phf::Map<&'static str, &[&str]> = phf_map! {
    "Alias" => &[
        "Alias", "Def", "Defs",
        "CSend", "Send", "Super", "ZSuper",
    ],
    "Const" => &[
        "Casgn", "Class", "Module",
        "Const"
    ],
    "CSend" => &[
        "Alias", "Def", "Defs",
        "CSend", "Send", "Super", "ZSuper",
    ],
    "Cvar" => &[
        "Cvasgn",
        "Cvar"
    ],
    "Gvar" => &[
        "Gvasgn",
        "Gvar"
    ],
    "Ivar" => &[
        "Ivasgn",
        "Ivar"
    ],
    "Lvar" => &[
        "Arg", "Kwarg", "Kwoptarg", "Kwrestarg", "Lvasgn", "MatchVar", "Optarg", "Restarg", "Shadowarg",
        "Lvar"
    ],
    "Send" => &[
        "Alias", "Def", "Defs",
        "CSend", "Send", "Super", "ZSuper",
    ],
    "Super" => &[
        "Alias", "Def", "Defs",
        "CSend", "Send", "Super", "ZSuper",
    ],
    "ZSuper" => &[
        "Alias", "Def", "Defs",
        "CSend", "Send", "Super", "ZSuper",
    ],
};

static ASSIGNMENT_TYPE_RESTRICTIONS: phf::Map<&'static str, &[&str]> = phf_map! {
    "Alias" => &[
        "Alias", "CSend", "Send", "Super", "ZSuper",
        "Def", "Defs"
    ],
    "Arg" => &[
        "Lvar",
        "Arg", "Kwarg", "Kwoptarg", "Kwrestarg", "Lvasgn", "MatchVar", "Optarg", "Restarg", "Shadowarg"
    ],
    "Casgn" => &[
        "Const",
        "Casgn", "Class", "Module"
    ],
    "Class" => &[
        "Const",
        "Casgn", "Class", "Module"
    ],
    "Cvasgn" => &[
        "Cvar",
        "Cvasgn"
    ],
    "Def" => &[
        "Alias", "CSend", "Send", "Super", "ZSuper",
        "Def"
    ],
    "Defs" => &[
        "Alias", "CSend", "Send", "Super", "ZSuper",
        "Defs"
    ],
    "Gvasgn" => &[
        "Gvar",
        "Gvasgn"
    ],
    "Ivasgn" => &[
        "Ivar",
        "Ivasgn"
    ],
    "Kwarg" => &[
        "Lvar",
        "Arg", "Kwarg", "Kwoptarg", "Kwrestarg", "Lvasgn", "MatchVar", "Optarg", "Restarg", "Shadowarg"
    ],
    "Kwoptarg" => &[
        "Lvar",
        "Arg", "Kwarg", "Kwoptarg", "Kwrestarg", "Lvasgn", "MatchVar", "Optarg", "Restarg", "Shadowarg"
    ],
    "Kwrestarg" => &[
        "Lvar",
        "Arg", "Kwarg", "Kwoptarg", "Kwrestarg", "Lvasgn", "MatchVar", "Optarg", "Restarg", "Shadowarg"
    ],
    "Lvasgn" => &[
        "Lvar",
        "Arg", "Kwarg", "Kwoptarg", "Kwrestarg", "Lvasgn", "MatchVar", "Optarg", "Restarg", "Shadowarg"
    ],
    "MatchVar" => &[
        "Lvar",
        "Arg", "Kwarg", "Kwoptarg", "Kwrestarg", "Lvasgn", "MatchVar", "Optarg", "Restarg", "Shadowarg"
    ],
    "Module" => &[
        "Const",
        "Casgn", "Class", "Module"
    ],
    "Optarg" => &[
        "Lvar",
        "Arg", "Kwarg", "Kwoptarg", "Kwrestarg", "Lvasgn", "MatchVar", "Optarg", "Restarg", "Shadowarg"
    ],
    "Restarg" => &[
        "Lvar",
        "Arg", "Kwarg", "Kwoptarg", "Kwrestarg", "Lvasgn", "MatchVar", "Optarg", "Restarg", "Shadowarg"
    ],
    "Shadowarg" => &[
        "Lvar",
        "Arg", "Kwarg", "Kwoptarg", "Kwrestarg", "Lvasgn", "MatchVar", "Optarg", "Restarg", "Shadowarg"
    ],
};

#[derive(Clone)]
pub struct IndexableDir {
    path: String,
    interface_only: bool,
}

pub struct Persistence {
    schema: Schema,
    schema_fields: SchemaFields,
    index: Option<Index>,
    workspace_path: String,
    last_reindex_time: i64,
    indexed_file_paths: HashSet<String>,
    process_id: Option<u32>,
    no_workspace: bool,
    gems_indexed: bool,
    include_dirs_indexed: bool,
    index_interface_only: bool,
    class_scope: Vec<String>,
    include_dirs: Vec<IndexableDir>,
    pub report_diagnostics: bool,
}

struct SchemaFields {
    file_path_id: Field,
    file_path: Field,
    category_field: Field,
    fuzzy_ruby_scope_field: Field,
    class_scope_field: Field,
    name_field: Field,
    node_type_field: Field,
    line_field: Field,
    start_column_field: Field,
    end_column_field: Field,
    columns_field: Field,
    user_space_field: Field,
}

#[derive(Debug)]
struct FuzzyNode<'a> {
    category: &'a str,
    fuzzy_ruby_scope: Vec<String>,
    class_scope: Vec<String>,
    name: String,
    node_type: &'a str,
    line: usize,
    start_column: usize,
    end_column: usize,
}

impl Persistence {
    pub fn new() -> tantivy::Result<Persistence> {
        let mut schema_builder = Schema::builder();
        let schema_fields = SchemaFields {
            file_path_id: schema_builder.add_text_field(
                "file_path_id",
                TextOptions::default()
                    .set_indexing_options(
                        TextFieldIndexing::default()
                            .set_tokenizer("raw")
                            .set_index_option(IndexRecordOption::Basic),
                    )
                    .set_stored(),
            ),
            file_path: schema_builder.add_text_field(
                "file_path",
                TextOptions::default()
                    .set_indexing_options(
                        TextFieldIndexing::default()
                            .set_tokenizer("raw")
                            .set_index_option(IndexRecordOption::Basic),
                    )
                    .set_stored(),
            ),
            category_field: schema_builder.add_text_field(
                "category",
                TextOptions::default()
                    .set_indexing_options(
                        TextFieldIndexing::default()
                            .set_tokenizer("raw")
                            .set_index_option(IndexRecordOption::Basic),
                    )
                    .set_stored(),
            ),
            fuzzy_ruby_scope_field: schema_builder.add_text_field(
                "fuzzy_ruby_scope",
                TextOptions::default()
                    .set_indexing_options(
                        TextFieldIndexing::default()
                            .set_tokenizer("raw")
                            .set_index_option(IndexRecordOption::Basic),
                    )
                    .set_stored(),
            ),
            class_scope_field: schema_builder.add_text_field(
                "class_scope",
                TextOptions::default()
                    .set_indexing_options(
                        TextFieldIndexing::default()
                            .set_tokenizer("raw")
                            .set_index_option(IndexRecordOption::Basic),
                    )
                    .set_stored(),
            ),
            name_field: schema_builder.add_text_field(
                "name",
                TextOptions::default()
                    .set_indexing_options(
                        TextFieldIndexing::default()
                            .set_tokenizer("raw")
                            .set_index_option(IndexRecordOption::Basic),
                    )
                    .set_stored(),
            ),
            node_type_field: schema_builder.add_text_field(
                "node_type",
                TextOptions::default()
                    .set_indexing_options(
                        TextFieldIndexing::default()
                            .set_tokenizer("raw")
                            .set_index_option(IndexRecordOption::Basic),
                    )
                    .set_stored(),
            ),
            line_field: schema_builder.add_u64_field("line", INDEXED | STORED),
            start_column_field: schema_builder.add_u64_field("start_column", INDEXED | STORED),
            end_column_field: schema_builder.add_u64_field("end_column", INDEXED | STORED),
            columns_field: schema_builder.add_u64_field("columns", INDEXED | STORED),
            user_space_field: schema_builder.add_bool_field("user_space", INDEXED | STORED),
        };

        let schema = schema_builder.build();
        let index = None;
        let workspace_path = "unset".to_string();
        let last_reindex_time = FileTime::from_unix_time(0, 0).seconds();
        let indexed_file_paths = HashSet::new();
        let process_id: Option<u32> = None;
        let no_workspace = false;
        let gems_indexed = false;
        let index_interface_only = false;
        let class_scope = vec![];
        let report_diagnostics = true;
        let include_dirs = Vec::new();
        let include_dirs_indexed = false;

        Ok(Self {
            schema,
            schema_fields,
            index,
            workspace_path,
            last_reindex_time,
            indexed_file_paths,
            process_id,
            no_workspace,
            gems_indexed,
            index_interface_only,
            class_scope,
            report_diagnostics,
            include_dirs,
            include_dirs_indexed,
        })
    }

    pub fn initialize(&mut self, params: &InitializeParams) {
        let uri = params.root_uri.as_ref().unwrap_or_else(|| {
            info!("root_uri wasn't given to initialize, exiting.");
            quit::with_code(1);
        });

        self.workspace_path = uri.path().to_string();

        let default_user_config = json!({});
        let default_allocation_type = json!("ram");

        let user_config = &params
            .initialization_options
            .as_ref()
            .unwrap_or(&default_user_config)
            .as_object()
            .unwrap();
        let allocation_type = user_config
            .get("allocationType")
            .unwrap_or(&default_allocation_type)
            .as_str()
            .unwrap();

        self.index = match allocation_type {
            "ram" => Some(Index::create_in_ram(self.schema.clone())),
            "tempdir" => Some(Index::create_from_tempdir(self.schema.clone()).unwrap()),
            _ => {
                info!("Unknown allocation_type, defaulting to tempdir");
                Some(Index::create_from_tempdir(self.schema.clone()).unwrap())
            }
        };

        if let Some(included_dirs) = user_config.get("includeDirs") {
            if let Some(dirs) = included_dirs.as_array() {
                let dirs = dirs
                    .iter()
                    .map(|v| {
                        // v.as_str().unwrap().to_string()
                        let dir_params = v.as_object().unwrap();
                        let dir_path = dir_params.get("path").unwrap().as_str().unwrap();
                        let interface_only = {
                            let param = dir_params.get("interface_only");
                            match param {
                                Some(val) => val.as_bool().unwrap(),
                                None => true,
                            }
                        };

                        let dir_path = dir_path.to_string();
                        let absolute_dir_path = if dir_path.starts_with("/") {
                            dir_path
                        } else {
                            format!("{}/{}", &self.workspace_path, dir_path)
                        };

                        IndexableDir {
                            path: absolute_dir_path,
                            interface_only,
                        }
                    })
                    .collect();

                self.include_dirs = dirs;
            };
        }

        let default_index_gems = json!(true);
        let skip_indexing_gems = !user_config
            .get("indexGems")
            .unwrap_or(&default_index_gems)
            .as_bool()
            .unwrap();
        if skip_indexing_gems {
            self.gems_indexed = true;
        }

        let default_report_diagnostics = json!(true);
        let report_diagnostics = user_config
            .get("reportDiagnostics")
            .unwrap_or(&default_report_diagnostics)
            .as_bool()
            .unwrap();
        if !report_diagnostics {
            self.report_diagnostics = false;
        }
    }

    pub fn reindex_modified_files(&mut self) -> tantivy::Result<()> {
        let start_time = FileTime::from_unix_time(FileTime::now().unix_seconds(), 0).seconds() - 1;
        let last_reindex_time = self.last_reindex_time.clone();

        let walk_dir = WalkDirGeneric::<(usize, bool)>::new(&self.workspace_path).process_read_dir(
            move |_depth, _path, _read_dir_state, children| {
                children.retain(|dir_entry_result| {
                    dir_entry_result
                        .as_ref()
                        .map(|dir_entry| {
                            if let Some(file_name) = dir_entry.file_name.to_str() {
                                let ruby_file = file_name.ends_with(".rb");
                                dir_entry.file_type.is_dir() || ruby_file
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false)
                });

                children.iter_mut().for_each(|dir_entry_result| {
                    if let Ok(dir_entry) = dir_entry_result {
                        if let Some(file_name) = dir_entry.file_name.to_str() {
                            if file_name.contains("node_modules")
                                || file_name.contains("tmp")
                                || file_name.contains(".git")
                            {
                                dir_entry.read_children_path = None;
                            }
                        }
                    }
                });
            },
        );

        let mut new_indexable_file_paths = HashSet::new();
        let mut indexed_file_paths = HashSet::new();

        for entry in walk_dir {
            let path = entry.unwrap().path();
            let path = path.to_str().unwrap();
            let ruby_file = path.ends_with(".rb");

            if ruby_file {
                indexed_file_paths.insert(path.to_string());
                self.indexed_file_paths.remove(path);

                let metadata = fs::metadata(path).unwrap();

                let mtime = FileTime::from_last_modification_time(&metadata);
                let recently_modified = mtime.seconds() >= last_reindex_time;

                if recently_modified {
                    new_indexable_file_paths.insert(path.to_string());
                }
            }
        }

        if let Some(index) = &self.index {
            let files_added = new_indexable_file_paths.len() > 0;
            let files_deleted = self.indexed_file_paths.len() > 0;

            if files_added || files_deleted {
                let mut index_writer = index.writer(256_000_000).unwrap();

                for path in &self.indexed_file_paths {
                    let relative_path = path.replace(&self.workspace_path, "");

                    let file_path_id = blake3::hash(&relative_path.as_bytes());
                    let path_term = Term::from_field_text(
                        self.schema_fields.file_path_id,
                        &file_path_id.to_string(),
                    );

                    index_writer.delete_term(path_term);
                }

                for path in &new_indexable_file_paths {
                    let text = fs::read_to_string(&path).unwrap();
                    let uri = Url::from_file_path(&path).unwrap();
                    let relative_path = uri.path().replace(&self.workspace_path, "");

                    self.reindex_modified_file_without_commit(
                        &text,
                        relative_path,
                        &index_writer,
                        true,
                    );
                }

                index_writer.commit().unwrap();
                info!("Indexing workspace complete!");
            } else {
                info!("No file changes, skipping periodic reindexing.")
            }
        }

        self.last_reindex_time = start_time;
        self.indexed_file_paths = indexed_file_paths;

        Ok(())
    }

    pub fn index_included_dirs_once(&mut self) -> tantivy::Result<()> {
        if self.include_dirs_indexed {
            return Ok(());
        }

        self.index_interface_only = true;

        if self.include_dirs.len() > 0 {
            let index = match &self.index {
                Some(index) => index,
                None => {
                    info!("missing index");
                    quit::with_code(1);
                }
            };

            let mut index_writer = index.writer(256_000_000).unwrap();

            for indexable_dir in self.include_dirs.clone() {
                let walk_dir = WalkDirGeneric::<(usize, bool)>::new(indexable_dir.path.clone())
                    .process_read_dir(move |_depth, _path, _read_dir_state, children| {
                        children.retain(|dir_entry_result| {
                            dir_entry_result
                                .as_ref()
                                .map(|dir_entry| {
                                    if let Some(file_name) = dir_entry.file_name.to_str() {
                                        let ruby_file = file_name.ends_with(".rb");
                                        dir_entry.file_type.is_dir() || ruby_file
                                    } else {
                                        false
                                    }
                                })
                                .unwrap_or(false)
                        });

                        children.iter_mut().for_each(|dir_entry_result| {
                            if let Ok(dir_entry) = dir_entry_result {
                                if let Some(file_name) = dir_entry.file_name.to_str() {
                                    if file_name.contains("node_modules")
                                        || file_name.contains("vendor")
                                        || file_name.contains("tmp")
                                        || file_name.contains(".git")
                                    {
                                        dir_entry.read_children_path = None;
                                    }
                                }
                            }
                        });
                    });

                let mut indexable_file_paths = Vec::new();

                for entry in walk_dir {
                    let path = entry.unwrap().path();
                    let path = path.to_str().unwrap();
                    let ruby_file = path.ends_with(".rb");

                    if ruby_file {
                        indexable_file_paths.push(path.to_string());
                    }
                }

                self.index_interface_only = indexable_dir.interface_only;

                for path in &indexable_file_paths {
                    if let Ok(text) = fs::read_to_string(&path) {
                        let uri = Url::from_file_path(&path).unwrap();
                        let relative_path = uri.path().replace(&self.workspace_path, "");

                        self.reindex_modified_file_without_commit(
                            &text,
                            relative_path,
                            &index_writer,
                            false,
                        );
                    }
                }
            }

            index_writer.commit().unwrap();
        }

        self.include_dirs_indexed = true;
        self.index_interface_only = false;

        Ok(())
    }

    pub fn index_gems_once(&mut self) -> tantivy::Result<()> {
        if self.gems_indexed {
            return Ok(());
        }

        self.index_interface_only = true;

        // Four leading spaces dictates that it's a gem version
        // https://github.com/rubygems/bundler/blob/v2.1.4/lib/bundler/lockfile_parser.rb#L174-L181
        let gem_version = Regex::new(r"^\s{4}([a-zA-Z\d\.\-_]+)\s\(([\d\w\.\-_]+)\)").unwrap();
        let gemfile_path = format!("{}/{}", &self.workspace_path, "Gemfile.lock");

        if let Ok(gemfile_contents) = fs::read_to_string(gemfile_path) {
            let mut gem_paths = vec![];
            let mut base_gem_path = "unset";

            let gem_home_path_result = Command::new("sh")
                .arg("-c")
                // .arg(format!("eval \"$(/usr/local/bin/rbenv init -)\" && cd {} && gem environment home", &self.workspace_path))
                .arg(format!(
                    "cd {} && gem environment home",
                    &self.workspace_path
                ))
                .output();

            if let Ok(gem_home_path) = gem_home_path_result {
                if let Ok(gem_home_path) = str::from_utf8(gem_home_path.stdout.as_slice()) {
                    base_gem_path = gem_home_path;
                }

                // Index Ruby
                let ruby_source_path = base_gem_path.replace("gems/", "").replace("\n", "");

                info!("Added Ruby source path: {}", ruby_source_path);
                gem_paths.push(ruby_source_path);

                // Index Gems
                for line in gemfile_contents.lines() {
                    if let Some(captures) = gem_version.captures(line) {
                        let name = captures[1].to_string();
                        let version = captures[2].to_string();
                        let gem_folder_name =
                            format!("{}/gems/{}-{}", base_gem_path, name, version);
                        // Not 100% sure where this newline is coming from. `gemfile_contents.lines()` I think.
                        let gem_folder_name = gem_folder_name.replace("\n", "");

                        info!("gem folder name: {}", gem_folder_name);

                        gem_paths.push(gem_folder_name)
                    }
                }
            }

            let index = match &self.index {
                Some(index) => index,
                None => {
                    info!("missing index");
                    quit::with_code(1);
                }
            };

            let mut index_writer = index.writer(256_000_000).unwrap();

            for gem_path in gem_paths {
                let walk_dir = WalkDirGeneric::<(usize, bool)>::new(gem_path.clone())
                    .process_read_dir(move |_depth, _path, _read_dir_state, children| {
                        children.retain(|dir_entry_result| {
                            dir_entry_result
                                .as_ref()
                                .map(|dir_entry| {
                                    if let Some(file_name) = dir_entry.file_name.to_str() {
                                        let ruby_file = file_name.ends_with(".rb");
                                        dir_entry.file_type.is_dir() || ruby_file
                                    } else {
                                        false
                                    }
                                })
                                .unwrap_or(false)
                        });

                        children.iter_mut().for_each(|dir_entry_result| {
                            if let Ok(dir_entry) = dir_entry_result {
                                if let Some(file_name) = dir_entry.file_name.to_str() {
                                    if file_name.contains("node_modules")
                                        || file_name.contains("vendor")
                                        || file_name.contains("tmp")
                                        || file_name.contains(".git")
                                    {
                                        dir_entry.read_children_path = None;
                                    }
                                }
                            }
                        });
                    });

                let mut indexable_file_paths = Vec::new();

                for entry in walk_dir {
                    let path = entry.unwrap().path();
                    let path = path.to_str().unwrap();
                    let ruby_file = path.ends_with(".rb");

                    if ruby_file {
                        indexable_file_paths.push(path.to_string());
                    }
                }

                for path in &indexable_file_paths {
                    if let Ok(text) = fs::read_to_string(&path) {
                        let uri = Url::from_file_path(&path).unwrap();
                        let relative_path = uri.path().replace(&self.workspace_path, "");

                        self.reindex_modified_file_without_commit(
                            &text,
                            relative_path,
                            &index_writer,
                            false,
                        );
                    }
                }
            }

            index_writer.commit().unwrap();
        } else {
            info!("Gemfile not found, skipping indexing workspace gems.");
        }

        self.gems_indexed = true;
        self.index_interface_only = false;

        Ok(())
    }

    pub fn reindex_modified_file_without_commit(
        &mut self,
        text: &String,
        relative_path: String,
        index_writer: &IndexWriter,
        user_space: bool,
    ) -> tantivy::Result<Vec<Option<tower_lsp::lsp_types::Diagnostic>>> {
        if let Some(_) = &self.index {
            let mut documents = Vec::new();

            let diagnostics = match self.parse(text, &mut documents) {
                Ok(diagnostics) => diagnostics,
                Err(diagnostics) => {
                    // Return early so existing documents are not deleted when
                    // there is a syntax error
                    return Ok(diagnostics);
                }
            };

            let file_path_id = blake3::hash(&relative_path.as_bytes());

            for document in documents {
                let mut fuzzy_doc = Document::default();

                fuzzy_doc.add_text(self.schema_fields.file_path_id, &file_path_id.to_string());

                for path_part in relative_path.split("/") {
                    if path_part.len() > 0 {
                        fuzzy_doc.add_text(self.schema_fields.file_path, path_part);
                    }
                }

                for fuzzy_scope in document.fuzzy_ruby_scope {
                    fuzzy_doc.add_text(self.schema_fields.fuzzy_ruby_scope_field, fuzzy_scope);
                }

                for class_scope in document.class_scope {
                    fuzzy_doc.add_text(self.schema_fields.class_scope_field, class_scope);
                }

                fuzzy_doc.add_text(
                    self.schema_fields.category_field,
                    document.category.to_string(),
                );
                fuzzy_doc.add_text(self.schema_fields.name_field, document.name);
                fuzzy_doc.add_text(self.schema_fields.node_type_field, document.node_type);
                fuzzy_doc.add_u64(
                    self.schema_fields.line_field,
                    document.line.try_into().unwrap(),
                );
                fuzzy_doc.add_u64(
                    self.schema_fields.start_column_field,
                    document.start_column.try_into().unwrap(),
                );
                fuzzy_doc.add_u64(
                    self.schema_fields.end_column_field,
                    document.end_column.try_into().unwrap(),
                );
                fuzzy_doc.add_bool(self.schema_fields.user_space_field, user_space);

                let start_col = document.start_column;
                let end_col = document.end_column;
                let col_range = start_col..(end_col + 1);
                for col in col_range {
                    fuzzy_doc.add_u64(self.schema_fields.columns_field, col as u64);
                }

                index_writer.add_document(fuzzy_doc)?;
            }

            Ok(diagnostics)
        } else {
            Ok(vec![])
        }
    }

    pub async fn reindex_modified_file(&mut self, client: &Client, text: &String, uri: &Url) {
        let mut documents = Vec::new();
        let diagnostics = match self.parse(text, &mut documents) {
            Ok(diagnostics) => diagnostics,
            Err(diagnostics) => {
                // Return early so existing documents are not deleted when
                // there is a syntax error
                // return Ok(diagnostics);
                diagnostics
            }
        };

        if self.report_diagnostics {
            let mut reported_diagnostics = vec![];

            for diagnostic in &diagnostics {
                for unwrapped_diagnostic in diagnostic {
                    reported_diagnostics.push(unwrapped_diagnostic.clone());
                }
            }

            client
                .publish_diagnostics(uri.clone(), reported_diagnostics, None)
                .await;
            // .await;
        }

        if diagnostics.len() > 0 {
            return;
        }

        if let Some(index) = &self.index {
            let mut index_writer = index.writer_with_num_threads(1, 30_000_000).unwrap();

            let user_space: bool;
            let relative_path: String;

            if uri.path().contains(&self.workspace_path) {
                user_space = true;
                relative_path = uri.path().replace(&self.workspace_path, "");
            } else {
                user_space = false;
                relative_path = uri.path().to_string();
            }

            let file_path_id = blake3::hash(&relative_path.as_bytes());

            let file_path_id_term =
                Term::from_field_text(self.schema_fields.file_path_id, &file_path_id.to_string());

            index_writer.delete_term(file_path_id_term);

            for document in documents {
                let mut fuzzy_doc = Document::default();

                fuzzy_doc.add_text(self.schema_fields.file_path_id, &file_path_id.to_string());

                for path_part in relative_path.split("/") {
                    if path_part.len() > 0 {
                        fuzzy_doc.add_text(self.schema_fields.file_path, path_part);
                    }
                }

                for fuzzy_scope in document.fuzzy_ruby_scope {
                    fuzzy_doc.add_text(self.schema_fields.fuzzy_ruby_scope_field, fuzzy_scope);
                }

                for class_scope in document.class_scope {
                    fuzzy_doc.add_text(self.schema_fields.class_scope_field, class_scope);
                }

                fuzzy_doc.add_text(
                    self.schema_fields.category_field,
                    document.category.to_string(),
                );
                fuzzy_doc.add_text(self.schema_fields.name_field, document.name);
                fuzzy_doc.add_text(self.schema_fields.node_type_field, document.node_type);
                fuzzy_doc.add_u64(
                    self.schema_fields.line_field,
                    document.line.try_into().unwrap(),
                );
                fuzzy_doc.add_u64(
                    self.schema_fields.start_column_field,
                    document.start_column.try_into().unwrap(),
                );
                fuzzy_doc.add_u64(
                    self.schema_fields.end_column_field,
                    document.end_column.try_into().unwrap(),
                );
                fuzzy_doc.add_bool(self.schema_fields.user_space_field, user_space);

                let start_col = document.start_column;
                let end_col = document.end_column;
                let col_range = start_col..(end_col + 1);
                for col in col_range {
                    fuzzy_doc.add_u64(self.schema_fields.columns_field, col as u64);
                }

                index_writer.add_document(fuzzy_doc).unwrap();
            }

            index_writer.commit().unwrap();
        }
    }

    pub fn diagnostics(
        &mut self,
        text: &String,
        _uri: &Url,
    ) -> tantivy::Result<Vec<Option<tower_lsp::lsp_types::Diagnostic>>> {
        let mut documents = Vec::new();
        match self.parse(text, &mut documents) {
            Ok(diagnostics) => Ok(diagnostics),
            Err(diagnostics) => Ok(diagnostics),
        }
    }

    pub fn find_definitions(
        &self,
        params: TextDocumentPositionParams,
    ) -> tantivy::Result<Vec<Location>> {
        let path = params.text_document.uri.path();
        let relative_path = path.replace(&self.workspace_path, "");

        let position = params.position;

        if let Some(index) = &self.index {
            let reader = index
                .reader_builder()
                .reload_policy(ReloadPolicy::OnCommit)
                .try_into()?;

            let searcher = reader.searcher();
            let character_position = position.character;
            let character_line = position.line;
            let file_path_id = blake3::hash(&relative_path.as_bytes());

            let file_path_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_text(self.schema_fields.file_path_id, &file_path_id.to_string()),
                IndexRecordOption::Basic,
            ));
            let category_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_text(self.schema_fields.category_field, "usage"),
                IndexRecordOption::Basic,
            ));
            let line_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_u64(self.schema_fields.line_field, character_line.into()),
                IndexRecordOption::Basic,
            ));
            let column_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_u64(self.schema_fields.columns_field, character_position.into()),
                IndexRecordOption::Basic,
            ));

            let query = BooleanQuery::new(vec![
                (Occur::Must, file_path_query),
                (Occur::Must, category_query),
                (Occur::Must, line_query),
                (Occur::Must, column_query),
            ]);

            let usage_top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;

            let mut locations = Vec::new();

            if usage_top_docs.len() == 0 {
                info!("No usages docs found");
                return Ok(locations);
            }

            let doc_address = usage_top_docs[0].1;
            let retrieved_doc = searcher.doc(doc_address)?;

            let category_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_text(self.schema_fields.category_field, "assignment"),
                IndexRecordOption::Basic,
            ));

            let usage_name = retrieved_doc
                .get_first(self.schema_fields.name_field)
                .unwrap()
                .as_text()
                .unwrap();
            let usage_type = retrieved_doc
                .get_first(self.schema_fields.node_type_field)
                .unwrap()
                .as_text()
                .unwrap();

            let name_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_text(self.schema_fields.name_field, usage_name),
                IndexRecordOption::Basic,
            ));

            let mut assignment_type_queries = vec![];

            for possible_assignment_type in USAGE_TYPE_RESTRICTIONS.get(usage_type).unwrap().iter()
            {
                let assignment_type_query: Box<dyn Query> = Box::new(TermQuery::new(
                    Term::from_field_text(
                        self.schema_fields.node_type_field,
                        possible_assignment_type,
                    ),
                    IndexRecordOption::Basic,
                ));

                assignment_type_queries.push((Occur::Should, assignment_type_query));
            }

            let assignment_type_query = BooleanQuery::new(assignment_type_queries);

            let mut queries = vec![
                (Occur::Must, category_query),
                (Occur::Must, name_query),
                (Occur::Must, Box::new(assignment_type_query)),
            ];

            let usage_fuzzy_scope =
                retrieved_doc.get_all(self.schema_fields.fuzzy_ruby_scope_field);

            match usage_type {
                // "Alias" => {},
                "Const" => {
                    for scope_name in usage_fuzzy_scope {
                        let scope_query: Box<dyn Query> = Box::new(TermQuery::new(
                            Term::from_field_text(
                                self.schema_fields.fuzzy_ruby_scope_field,
                                scope_name.as_text().unwrap(),
                            ),
                            IndexRecordOption::Basic,
                        ));

                        queries.push((Occur::Should, scope_query));
                    }

                    let class_scope = retrieved_doc.get_all(self.schema_fields.class_scope_field);

                    for scope_name in class_scope {
                        let scope_query: Box<dyn Query> = Box::new(TermQuery::new(
                            Term::from_field_text(
                                self.schema_fields.fuzzy_ruby_scope_field,
                                scope_name.as_text().unwrap(),
                            ),
                            IndexRecordOption::Basic,
                        ));

                        queries.push((Occur::Must, scope_query));
                    }
                }
                // "CSend" => {},
                // todo: improved indexed scopes so there is a separate class scope, etc
                // "Cvar" => {},
                // "Gvar" => {},
                // todo: improved indexed scopes so there is a separate class scope, etc
                // "Ivar" => {},
                // todo: improved to be more accurate
                "Arg" | "Kwarg" | "Kwoptarg" | "Kwrestarg" | "Lvasgn" | "MatchVar" | "Optarg"
                | "Restarg" | "Shadowarg" | "Lvar" => {
                    for scope_name in usage_fuzzy_scope {
                        let scope_query: Box<dyn Query> = Box::new(TermQuery::new(
                            Term::from_field_text(
                                self.schema_fields.fuzzy_ruby_scope_field,
                                scope_name.as_text().unwrap(),
                            ),
                            IndexRecordOption::Basic,
                        ));

                        queries.push((Occur::Must, scope_query));
                    }
                }
                //
                "Send" => {
                    let class_scope = retrieved_doc.get_all(self.schema_fields.class_scope_field);

                    let mut usage_scope_fallback = true;

                    for scope_name in class_scope {
                        usage_scope_fallback = false;

                        let scope_query = Box::new(TermQuery::new(
                            Term::from_field_text(
                                self.schema_fields.fuzzy_ruby_scope_field,
                                scope_name.as_text().unwrap(),
                            ),
                            IndexRecordOption::Basic,
                        ));

                        let boosted_scope_query: Box<dyn Query> =
                            Box::new(BoostQuery::new(scope_query, 10000.0));

                        // queries.push((Occur::Should, scope_query));
                        // queries.push((Occur::Should, boosted_scope_query));

                        // This probably would be better as just a boosted
                        // query, but it's not working for some reason.
                        queries.push((Occur::Must, boosted_scope_query));
                    }

                    if usage_scope_fallback {
                        for scope_name in usage_fuzzy_scope {
                            let scope_query: Box<dyn Query> = Box::new(TermQuery::new(
                                Term::from_field_text(
                                    self.schema_fields.fuzzy_ruby_scope_field,
                                    scope_name.as_text().unwrap(),
                                ),
                                IndexRecordOption::Basic,
                            ));

                            queries.push((Occur::Should, scope_query));
                        }
                    }
                }
                // "Super" => {},
                // "ZSuper" => {},
                _ => {
                    for scope_name in usage_fuzzy_scope {
                        let scope_query: Box<dyn Query> = Box::new(TermQuery::new(
                            Term::from_field_text(
                                self.schema_fields.fuzzy_ruby_scope_field,
                                scope_name.as_text().unwrap(),
                            ),
                            IndexRecordOption::Basic,
                        ));

                        queries.push((Occur::Should, scope_query));
                    }
                }
            };

            let query = BooleanQuery::new(queries);
            let assignments_top_docs = searcher.search(&query, &TopDocs::with_limit(50))?;

            for (_score, doc_address) in assignments_top_docs {
                let retrieved_doc = searcher.doc(doc_address)?;

                let file_path: String = retrieved_doc
                    .get_all(self.schema_fields.file_path)
                    .flat_map(Value::as_text)
                    .collect::<Vec<&str>>()
                    .join("/");

                let absolute_file_path: String;

                let user_space = retrieved_doc
                    .get_first(self.schema_fields.user_space_field)
                    .unwrap()
                    .as_bool()
                    .unwrap() as bool;

                if user_space {
                    absolute_file_path = format!("{}/{}", &self.workspace_path, &file_path);
                } else {
                    absolute_file_path = format!("/{}", &file_path);
                }

                let doc_uri = Url::from_file_path(&absolute_file_path).unwrap();

                let start_line = retrieved_doc
                    .get_first(self.schema_fields.line_field)
                    .unwrap()
                    .as_u64()
                    .unwrap() as u32;
                let start_column = retrieved_doc
                    .get_first(self.schema_fields.start_column_field)
                    .unwrap()
                    .as_u64()
                    .unwrap() as u32;
                let start_position = Position::new(start_line, start_column);
                let end_column = retrieved_doc
                    .get_first(self.schema_fields.end_column_field)
                    .unwrap()
                    .as_u64()
                    .unwrap() as u32;
                let end_position = Position::new(start_line, end_column);

                let doc_range = Range::new(start_position, end_position);
                let location = Location::new(doc_uri, doc_range);

                locations.push(location);
            }

            Ok(locations)
        } else {
            Ok(vec![])
        }
    }

    pub fn find_highlights(
        &self,
        params: TextDocumentPositionParams,
    ) -> tantivy::Result<Vec<DocumentHighlight>> {
        if let Ok(search_results) = self.find_references(params) {
            let mut highlights = Vec::new();

            for search_result in &search_results {
                let start_line = search_result
                    .get_first(self.schema_fields.line_field)
                    .unwrap()
                    .as_u64()
                    .unwrap() as u32;
                let start_column = search_result
                    .get_first(self.schema_fields.start_column_field)
                    .unwrap()
                    .as_u64()
                    .unwrap() as u32;
                let start_position = Position::new(start_line, start_column);
                let end_column = search_result
                    .get_first(self.schema_fields.end_column_field)
                    .unwrap()
                    .as_u64()
                    .unwrap() as u32;
                let end_position = Position::new(start_line, end_column);

                let range = Range::new(start_position, end_position);

                let category = search_result
                    .get_first(self.schema_fields.category_field)
                    .unwrap()
                    .as_text()
                    .unwrap();

                let kind = if category == "assignment" {
                    Some(DocumentHighlightKind::WRITE)
                } else {
                    Some(DocumentHighlightKind::READ)
                };

                let document_highlight = DocumentHighlight { range, kind };

                highlights.push(document_highlight);
            }

            Ok(highlights)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn find_references(
        &self,
        params: TextDocumentPositionParams,
    ) -> tantivy::Result<Vec<Document>> {
        let path = params.text_document.uri.path();
        let relative_path = path.replace(&self.workspace_path, "");

        let position = params.position;

        if let Some(index) = &self.index {
            let reader = index
                .reader_builder()
                .reload_policy(ReloadPolicy::OnCommit)
                .try_into()?;

            let searcher = reader.searcher();
            let character_position = position.character;
            let character_line = position.line;
            let file_path_id = blake3::hash(&relative_path.as_bytes());

            let file_path_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_text(self.schema_fields.file_path_id, &file_path_id.to_string()),
                IndexRecordOption::Basic,
            ));
            let line_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_u64(self.schema_fields.line_field, character_line.into()),
                IndexRecordOption::Basic,
            ));
            let column_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_u64(self.schema_fields.columns_field, character_position.into()),
                IndexRecordOption::Basic,
            ));

            let query = BooleanQuery::new(vec![
                (Occur::Must, file_path_query),
                (Occur::Must, line_query),
                (Occur::Must, column_query),
            ]);

            let usage_top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;

            if usage_top_docs.len() == 0 {
                info!("No highlight usages docs found");
                return Ok(Vec::new());
            }

            let doc_address = usage_top_docs[0].1;
            let retrieved_doc = searcher.doc(doc_address)?;

            let usage_name = retrieved_doc
                .get_first(self.schema_fields.name_field)
                .unwrap()
                .as_text()
                .unwrap();
            let token_type = retrieved_doc
                .get_first(self.schema_fields.node_type_field)
                .unwrap()
                .as_text()
                .unwrap();

            let file_path_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_text(self.schema_fields.file_path_id, &file_path_id.to_string()),
                IndexRecordOption::Basic,
            ));

            let name_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_text(self.schema_fields.name_field, usage_name),
                IndexRecordOption::Basic,
            ));

            let mut highlight_token_queries = vec![];

            for possible_assignment_type in USAGE_TYPE_RESTRICTIONS
                .get(token_type)
                .unwrap_or(&[].as_slice())
                .iter()
            {
                let assignment_type_query: Box<dyn Query> = Box::new(TermQuery::new(
                    Term::from_field_text(
                        self.schema_fields.node_type_field,
                        possible_assignment_type,
                    ),
                    IndexRecordOption::Basic,
                ));

                highlight_token_queries.push((Occur::Should, assignment_type_query));
            }
            for possible_usage_type in ASSIGNMENT_TYPE_RESTRICTIONS
                .get(token_type)
                .unwrap_or(&[].as_slice())
                .iter()
            {
                let usage_type_query: Box<dyn Query> = Box::new(TermQuery::new(
                    Term::from_field_text(self.schema_fields.node_type_field, possible_usage_type),
                    IndexRecordOption::Basic,
                ));

                highlight_token_queries.push((Occur::Should, usage_type_query));
            }

            let token_type_query = BooleanQuery::new(highlight_token_queries);

            let mut queries = vec![
                (Occur::Must, file_path_query),
                (Occur::Must, name_query),
                (Occur::Must, Box::new(token_type_query)),
            ];

            let usage_fuzzy_scope =
                retrieved_doc.get_all(self.schema_fields.fuzzy_ruby_scope_field);

            match token_type {
                // "Alias" => {},
                // "Const" => {},
                // "CSend" => {},
                // todo: improved indexed scopes so there is a separate class scope, etc
                // "Cvar" => {},
                // "Gvar" => {},
                // todo: improved indexed scopes so there is a separate class scope, etc
                // "Ivar" => {},
                // todo: improved to be more accurate

                // same values as local assignment type restrictions, for
                // example "Lvasgn" in ASSIGNMENT_TYPE_RESTRICTIONS
                "Arg" | "Kwarg" | "Kwoptarg" | "Kwrestarg" | "Lvasgn" | "MatchVar" | "Optarg"
                | "Restarg" | "Shadowarg" | "Lvar" => {
                    for scope_name in usage_fuzzy_scope {
                        let scope_query: Box<dyn Query> = Box::new(TermQuery::new(
                            Term::from_field_text(
                                self.schema_fields.fuzzy_ruby_scope_field,
                                scope_name.as_text().unwrap(),
                            ),
                            IndexRecordOption::Basic,
                        ));

                        queries.push((Occur::Must, scope_query));
                    }
                }
                // "Send" => {},
                // "Super" => {},
                // "ZSuper" => {},
                _ => {
                    for scope_name in usage_fuzzy_scope {
                        let scope_query: Box<dyn Query> = Box::new(TermQuery::new(
                            Term::from_field_text(
                                self.schema_fields.fuzzy_ruby_scope_field,
                                scope_name.as_text().unwrap(),
                            ),
                            IndexRecordOption::Basic,
                        ));

                        queries.push((Occur::Should, scope_query));
                    }
                }
            };

            let results =
                searcher.search(&BooleanQuery::new(queries), &TopDocs::with_limit(100))?;

            let mut documents = Vec::new();

            for (_score, doc_address) in results {
                documents.push(searcher.doc(doc_address).unwrap())
            }

            Ok(documents)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn find_references_in_workspace(
        &self,
        query: String,
    ) -> tantivy::Result<Vec<Document>> {
        if let Some(index) = &self.index {
            let reader = index
                .reader_builder()
                .reload_policy(ReloadPolicy::OnCommit)
                .try_into()?;

            let searcher = reader.searcher();

            let user_space_query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_bool(self.schema_fields.user_space_field, true),
                IndexRecordOption::Basic,
            ));

            let name_query: Box<dyn Query> = Box::new(RegexQuery::from_pattern(
                format!("{}.*", query).as_str(),
                self.schema_fields.name_field,
            )?);

            let mut allowed_type_queries = vec![];
            let allowed_types = ["Alias", "Casgn", "Class", "Def", "Defs", "Gvasgn", "Module"];

            for allowed_type in allowed_types {
                let assignment_type_query: Box<dyn Query> = Box::new(TermQuery::new(
                    Term::from_field_text(self.schema_fields.node_type_field, allowed_type),
                    IndexRecordOption::Basic,
                ));

                allowed_type_queries.push((Occur::Should, assignment_type_query));
            }

            let allowed_types_query = BooleanQuery::new(allowed_type_queries);

            let queries = vec![
                (Occur::Must, user_space_query),
                (Occur::Must, name_query),
                (Occur::Must, Box::new(allowed_types_query)),
            ];

            let results =
                searcher.search(&BooleanQuery::new(queries), &TopDocs::with_limit(100))?;

            let mut documents = Vec::new();

            for (_score, doc_address) in results {
                documents.push(searcher.doc(doc_address).unwrap())
            }

            Ok(documents)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn documents_to_locations(
        &self,
        path: &str,
        documents: Vec<Document>,
    ) -> Vec<Location> {
        let mut locations = Vec::new();

        for document in documents {
            let doc_uri = Url::from_file_path(path).unwrap();

            let start_line = document
                .get_first(self.schema_fields.line_field)
                .unwrap()
                .as_u64()
                .unwrap() as u32;
            let start_column = document
                .get_first(self.schema_fields.start_column_field)
                .unwrap()
                .as_u64()
                .unwrap() as u32;
            let start_position = Position::new(start_line, start_column);
            let end_column = document
                .get_first(self.schema_fields.end_column_field)
                .unwrap()
                .as_u64()
                .unwrap() as u32;
            let end_position = Position::new(start_line, end_column);

            let doc_range = Range::new(start_position, end_position);
            let location = Location::new(doc_uri, doc_range);

            locations.push(location);
        }

        locations
    }

    pub fn rename_tokens(
        &self,
        path: &str,
        documents: Vec<Document>,
        new_name: &String,
    ) -> WorkspaceEdit {
        let mut edits = Vec::new();

        for document in documents {
            let start_line = document
                .get_first(self.schema_fields.line_field)
                .unwrap()
                .as_u64()
                .unwrap() as u32;
            let start_column = document
                .get_first(self.schema_fields.start_column_field)
                .unwrap()
                .as_u64()
                .unwrap() as u32;
            let start_position = Position::new(start_line, start_column);
            let end_column = document
                .get_first(self.schema_fields.end_column_field)
                .unwrap()
                .as_u64()
                .unwrap() as u32;
            let end_position = Position::new(start_line, end_column);

            edits.push(TextEdit::new(
                Range::new(start_position, end_position),
                new_name.clone(),
            ));
        }

        let mut map = HashMap::new();
        let uri = Url::from_file_path(&path).unwrap();

        map.insert(uri, edits);

        let workspace_edit = WorkspaceEdit::new(map);

        workspace_edit
    }

    pub fn documents_to_symbol_information(
        &self,
        documents: Vec<Document>,
    ) -> Vec<SymbolInformation> {
        let mut symbol_infos = Vec::new();

        for document in documents {
            let doc_path: Vec<&str> = document
                .get_all(self.schema_fields.file_path)
                .map(|v| v.as_text().unwrap())
                .collect();
            let doc_path = doc_path.join("/");
            let absolute_file_path = format!("{}/{}", &self.workspace_path, &doc_path);
            let doc_uri = Url::from_file_path(absolute_file_path).unwrap();

            let name = document
                .get_first(self.schema_fields.name_field)
                .unwrap()
                .as_text()
                .unwrap();

            let start_line = document
                .get_first(self.schema_fields.line_field)
                .unwrap()
                .as_u64()
                .unwrap() as u32;
            let start_column = document
                .get_first(self.schema_fields.start_column_field)
                .unwrap()
                .as_u64()
                .unwrap() as u32;
            let start_position = Position::new(start_line, start_column);
            let end_column = document
                .get_first(self.schema_fields.end_column_field)
                .unwrap()
                .as_u64()
                .unwrap() as u32;
            let end_position = Position::new(start_line, end_column);

            let doc_type = document
                .get_first(self.schema_fields.node_type_field)
                .unwrap()
                .as_text()
                .unwrap();

            let symbol_kind = match doc_type {
                "Alias" => SymbolKind::METHOD,
                "Casgn" => SymbolKind::CLASS,
                "Class" => SymbolKind::CLASS,
                "Def" => SymbolKind::METHOD,
                "Defs" => SymbolKind::METHOD,
                "Gvasgn" => SymbolKind::VARIABLE,
                "Module" => SymbolKind::MODULE,
                _ => SymbolKind::VARIABLE,
            };

            let doc_range = Range::new(start_position, end_position);
            let symbol_location = Location::new(doc_uri, doc_range);

            let symbol_info = SymbolInformation {
                name: name.to_string(),
                kind: symbol_kind,
                tags: None,
                deprecated: None,
                location: symbol_location,
                container_name: None,
            };

            symbol_infos.push(symbol_info);
        }

        symbol_infos
    }

    fn parse(
        &mut self,
        contents: &String,
        documents: &mut Vec<FuzzyNode>,
    ) -> Result<
        Vec<Option<tower_lsp::lsp_types::Diagnostic>>,
        Vec<Option<tower_lsp::lsp_types::Diagnostic>>,
    > {
        let options = ParserOptions {
            buffer_name: "(eval)".to_string(),
            record_tokens: false,
            ..Default::default()
        };
        let parser = Parser::new(contents.to_string(), options);
        let parser_result = parser.do_parse();
        let input = parser_result.input;

        let mut diagnostics = vec![];

        for parser_diagnostic in parser_result.diagnostics {
            diagnostics.push(self.lsp_diagnostic(parser_diagnostic, &input));
        }

        let ast = match parser_result.ast {
            Some(a) => *a,
            None => return Err(diagnostics),
        };

        let mut scope = Vec::new();

        self.serialize(&ast, documents, &mut scope, &input);

        Ok(diagnostics)
    }

    fn lsp_diagnostic(
        &mut self,
        parser_diagnostic: lib_ruby_parser::Diagnostic,
        input: &DecodedInput,
    ) -> Option<tower_lsp::lsp_types::Diagnostic> {
        let diagnostic = || -> Option<tower_lsp::lsp_types::Diagnostic> {
            let (begin_lineno, start_column) =
                input.line_col_for_pos(parser_diagnostic.loc.begin).unwrap();
            let (end_lineno, end_column) =
                input.line_col_for_pos(parser_diagnostic.loc.end).unwrap();
            let start_position = Position::new(
                begin_lineno.try_into().unwrap(),
                start_column.try_into().unwrap(),
            );
            let end_position = Position::new(
                end_lineno.try_into().unwrap(),
                end_column.try_into().unwrap(),
            );

            Some(tower_lsp::lsp_types::Diagnostic::new_simple(
                Range::new(start_position, end_position),
                parser_diagnostic.message.render(),
            ))
        }();

        diagnostic
    }

    fn serialize(
        &mut self,
        node: &Node,
        documents: &mut Vec<FuzzyNode>,
        fuzzy_scope: &mut Vec<String>,
        input: &DecodedInput,
    ) {
        match &node {
            Node::Alias(Alias { to, from, .. }) => {
                if let Node::Sym(sym) = *to.to_owned() {
                    let (lineno, begin_pos) =
                        input.line_col_for_pos(sym.expression_l.begin).unwrap();
                    let (_lineno, end_pos) = input.line_col_for_pos(sym.expression_l.end).unwrap();

                    documents.push(FuzzyNode {
                        category: "assignment",
                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                        class_scope: vec![],
                        name: sym.name.to_string_lossy(),
                        node_type: "Alias",
                        line: lineno,
                        start_column: begin_pos,
                        end_column: end_pos,
                    });
                }

                if let Node::Sym(sym) = *from.to_owned() {
                    let (lineno, begin_pos) =
                        input.line_col_for_pos(sym.expression_l.begin).unwrap();
                    let (_lineno, end_pos) = input.line_col_for_pos(sym.expression_l.end).unwrap();

                    documents.push(FuzzyNode {
                        category: "usage",
                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                        class_scope: vec![],
                        name: sym.name.to_string_lossy(),
                        node_type: "Alias",
                        line: lineno,
                        start_column: begin_pos,
                        end_column: end_pos,
                    });
                }
            }

            Node::And(And { lhs, rhs, .. }) => {
                self.serialize(lhs, documents, fuzzy_scope, input);
                self.serialize(rhs, documents, fuzzy_scope, input);
            }

            Node::AndAsgn(AndAsgn { recv, value, .. }) => {
                self.serialize(recv, documents, fuzzy_scope, input);
                self.serialize(value, documents, fuzzy_scope, input);
            }

            Node::Arg(Arg { name, expression_l }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(expression_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(expression_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Arg",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });
            }

            Node::Args(Args { args, .. }) => {
                if self.index_interface_only {
                    return;
                }

                for node in args {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::Array(Array { elements, .. }) => {
                for node in elements {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::ArrayPattern(ArrayPattern { elements, .. }) => {
                for node in elements {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::ArrayPatternWithTail(ArrayPatternWithTail { elements, .. }) => {
                for node in elements {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            // Node::BackRef(BackRef { .. }) => {}
            Node::Begin(Begin { statements, .. }) => {
                for child_node in statements {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Block(Block {
                call, args, body, ..
            }) => {
                if self.index_interface_only {
                    return;
                }

                self.serialize(call, documents, fuzzy_scope, input);

                for child_node in args {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = body {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            // Node::Blockarg(Blockarg { .. }) => {}
            Node::BlockPass(BlockPass { value, .. }) => {
                if let Some(child_node) = value {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Break(Break { args, .. }) => {
                for child_node in args {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Case(Case {
                expr,
                when_bodies,
                else_body,
                ..
            }) => {
                if let Some(child_node) = expr {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                for child_node in when_bodies {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = else_body {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::CaseMatch(CaseMatch {
                expr,
                in_bodies,
                else_body,
                ..
            }) => {
                self.serialize(expr, documents, fuzzy_scope, input);

                for child_node in in_bodies {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = else_body {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Casgn(Casgn {
                scope,
                name,
                value,
                name_l,
                ..
            }) => {
                let const_node = Const {
                    scope: scope.to_owned(),
                    name: "".to_string(),
                    double_colon_l: None,
                    name_l: Loc { begin: 0, end: 0 },
                    expression_l: Loc { begin: 0, end: 0 },
                };
                let node_class_scope = self.build_class_scope(&const_node);

                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: node_class_scope,
                    name: name.to_string(),
                    node_type: "Casgn",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });

                if let Some(child_node) = scope {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = value {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            // Node::Cbase(Cbase { .. }) => {}
            Node::Class(Class {
                name,
                superclass,
                body,
                ..
            }) => {
                if let Node::Const(const_node) = *name.to_owned() {
                    // loop over names and add to fuzzy/class_scope
                    let node_class_scope = self.build_class_scope(&const_node);
                    let class_scope_len = node_class_scope.len();

                    for ancestor_name in node_class_scope {
                        fuzzy_scope.push(ancestor_name);
                    }

                    let (lineno, begin_pos) = input
                        .line_col_for_pos(const_node.expression_l.begin)
                        .unwrap();
                    let (_lineno, end_pos) =
                        input.line_col_for_pos(const_node.expression_l.end).unwrap();
                    let class_name = const_node.name.to_string();

                    let document = FuzzyNode {
                        category: "assignment",
                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                        // class_scope: node_class_scope,
                        class_scope: vec![],
                        name: class_name.clone(),
                        node_type: "Class",
                        line: lineno,
                        start_column: begin_pos,
                        end_column: end_pos,
                    };

                    documents.push(document);

                    fuzzy_scope.push(class_name.to_string());
                    self.class_scope.push(class_name);

                    if let Some(scope_node) = const_node.scope {
                        self.serialize(&scope_node, documents, fuzzy_scope, input);
                    }

                    if let Some(superclass_node) = superclass {
                        self.serialize(superclass_node, documents, fuzzy_scope, input);
                    }

                    for child_node in body {
                        self.serialize(child_node, documents, fuzzy_scope, input);
                    }

                    for _ in 0..class_scope_len {
                        fuzzy_scope.pop();
                    }

                    fuzzy_scope.pop();
                    self.class_scope.pop();
                }
            }

            // Node::Complex(Complex { .. }) => {}
            Node::Const(Const {
                scope,
                name,
                name_l,
                ..
            }) => {
                let const_node = Const {
                    scope: scope.to_owned(),
                    name: "".to_string(),
                    double_colon_l: None,
                    name_l: Loc { begin: 0, end: 0 },
                    expression_l: Loc { begin: 0, end: 0 },
                };
                let node_class_scope = self.build_class_scope(&const_node);

                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                let document = FuzzyNode {
                    category: "usage",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: node_class_scope,
                    name: name.to_string(),
                    node_type: "Const",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                };

                documents.push(document);

                if let Some(child_node) = scope {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::ConstPattern(ConstPattern {
                const_, pattern, ..
            }) => {
                self.serialize(const_, documents, fuzzy_scope, input);
                self.serialize(pattern, documents, fuzzy_scope, input);
            }

            Node::CSend(CSend {
                recv,
                method_name,
                args,
                selector_l,
                ..
            }) => {
                if let Some(loc) = selector_l {
                    let (lineno, begin_pos) = input.line_col_for_pos(loc.begin).unwrap();
                    let (_lineno, end_pos) = input.line_col_for_pos(loc.end).unwrap();

                    documents.push(FuzzyNode {
                        category: "usage",
                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                        class_scope: vec![],
                        name: method_name.to_string(),
                        node_type: "CSend",
                        line: lineno,
                        start_column: begin_pos,
                        end_column: end_pos,
                    });
                }

                self.serialize(recv, documents, fuzzy_scope, input);

                for child_node in args {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Cvar(Cvar { name, expression_l }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(expression_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(expression_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "usage",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Cvar",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });
            }

            Node::Cvasgn(Cvasgn {
                name,
                value,
                name_l,
                ..
            }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Cvasgn",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });

                if let Some(child_node) = value {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Def(Def {
                name,
                args,
                body,
                name_l,
                ..
            }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Def",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });

                if self.index_interface_only {
                    return;
                }

                fuzzy_scope.push(name.to_string());

                if let Some(child_node) = args {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = body {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                fuzzy_scope.pop();
            }

            Node::Defined(Defined { value, .. }) => {
                self.serialize(value, documents, fuzzy_scope, input);
            }

            Node::Defs(Defs {
                name,
                args,
                body,
                name_l,
                ..
            }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Defs",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });

                if self.index_interface_only {
                    return;
                }

                let mut scope_name = "self.".to_owned();
                scope_name.push_str(name);

                fuzzy_scope.push(scope_name);

                if let Some(child_node) = args {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = body {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                fuzzy_scope.pop();
            }

            Node::Dstr(Dstr { parts, .. }) => {
                for child_node in parts {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Dsym(Dsym { parts, .. }) => {
                for child_node in parts {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::EFlipFlop(EFlipFlop { left, right, .. }) => {
                if let Some(child_node) = left {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = right {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            // Node::EmptyElse(EmptyElse { .. }) => {}
            // Node::Encoding(Encoding { .. }) => {}
            Node::Ensure(Ensure { body, ensure, .. }) => {
                if let Some(child_node) = body {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = ensure {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Erange(Erange { left, right, .. }) => {
                if let Some(child_node) = left {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = right {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            // Node::False(False { .. }) => {}
            // Node::File(File { .. }) => {}
            Node::FindPattern(FindPattern { elements, .. }) => {
                for child_node in elements {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            // Node::Float(Float { .. }) => {}
            Node::For(For {
                iterator,
                iteratee,
                body,
                ..
            }) => {
                self.serialize(iterator, documents, fuzzy_scope, input);
                self.serialize(iteratee, documents, fuzzy_scope, input);

                for child_node in body {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            // Node::ForwardArg(ForwardArg { .. }) => {}
            // Node::ForwardedArgs(ForwardedArgs { .. }) => {}
            Node::Gvar(Gvar { name, expression_l }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(expression_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(expression_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "usage",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Gvar",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });
            }

            Node::Gvasgn(Gvasgn {
                name,
                value,
                name_l,
                ..
            }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Gvasgn",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });

                if let Some(child_node) = value {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Hash(Hash { pairs, .. }) => {
                for child_node in pairs {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::HashPattern(HashPattern { elements, .. }) => {
                for child_node in elements {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Heredoc(Heredoc { parts, .. }) => {
                for child_node in parts {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::If(If {
                cond,
                if_true,
                if_false,
                ..
            }) => {
                self.serialize(cond, documents, fuzzy_scope, input);

                if let Some(child_node) = if_true {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = if_false {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::IfGuard(IfGuard { cond, .. }) => {
                self.serialize(cond, documents, fuzzy_scope, input);
            }

            Node::IFlipFlop(IFlipFlop { left, right, .. }) => {
                if let Some(child_node) = left {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = right {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::IfMod(IfMod {
                cond,
                if_true,
                if_false,
                ..
            }) => {
                self.serialize(cond, documents, fuzzy_scope, input);

                if let Some(child_node) = if_true {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = if_false {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::IfTernary(IfTernary {
                cond,
                if_true,
                if_false,
                ..
            }) => {
                self.serialize(cond, documents, fuzzy_scope, input);
                self.serialize(if_true, documents, fuzzy_scope, input);
                self.serialize(if_false, documents, fuzzy_scope, input);
            }

            Node::Index(lib_ruby_parser::nodes::Index { recv, indexes, .. }) => {
                self.serialize(recv, documents, fuzzy_scope, input);

                for child_node in indexes {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::IndexAsgn(IndexAsgn {
                recv,
                indexes,
                value,
                ..
            }) => {
                self.serialize(recv, documents, fuzzy_scope, input);

                for child_node in indexes {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = value {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::InPattern(InPattern {
                pattern,
                guard,
                body,
                ..
            }) => {
                self.serialize(pattern, documents, fuzzy_scope, input);

                if let Some(child_node) = guard {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = body {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            // Node::Int(Int { .. }) => {}
            Node::Irange(Irange { left, right, .. }) => {
                if let Some(child_node) = left {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }

                if let Some(child_node) = right {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Ivar(Ivar { name, expression_l }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(expression_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(expression_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "usage",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Ivar",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });
            }

            Node::Ivasgn(Ivasgn {
                name,
                value,
                name_l,
                ..
            }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Ivasgn",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });

                if let Some(child_node) = value {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Kwarg(Kwarg { name, name_l, .. }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Kwarg",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });
            }

            Node::Kwargs(Kwargs { pairs, .. }) => {
                for node in pairs {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::KwBegin(KwBegin { statements, .. }) => {
                for node in statements {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            // Node::Kwnilarg(Kwnilarg { .. }) => {}
            Node::Kwoptarg(Kwoptarg {
                name,
                default,
                name_l,
                ..
            }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Kwoptarg",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });

                self.serialize(default, documents, fuzzy_scope, input);
            }

            Node::Kwrestarg(Kwrestarg { name, name_l, .. }) => {
                if let Some(node_name) = name {
                    if let Some(loc) = name_l {
                        let (lineno, begin_pos) = input.line_col_for_pos(loc.begin).unwrap();
                        let (_lineno, end_pos) = input.line_col_for_pos(loc.end).unwrap();

                        documents.push(FuzzyNode {
                            category: "assignment",
                            fuzzy_ruby_scope: fuzzy_scope.clone(),
                            class_scope: vec![],
                            name: node_name.to_string(),
                            node_type: "Kwrestarg",
                            line: lineno,
                            start_column: begin_pos,
                            end_column: end_pos,
                        });
                    }
                }
            }

            Node::Kwsplat(Kwsplat { value, .. }) => {
                self.serialize(value, documents, fuzzy_scope, input);
            }

            // Node::Lambda(Lambda { .. }) => {}
            // Node::Line(Line { .. }) => {}
            Node::Lvar(Lvar { name, expression_l }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(expression_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(expression_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "usage",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Lvar",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });
            }

            Node::Lvasgn(Lvasgn {
                name,
                value,
                name_l,
                ..
            }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Lvasgn",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });

                if let Some(child_node) = value {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::Masgn(Masgn { lhs, rhs, .. }) => {
                self.serialize(lhs, documents, fuzzy_scope, input);
                self.serialize(rhs, documents, fuzzy_scope, input);
            }

            Node::MatchAlt(MatchAlt { lhs, rhs, .. }) => {
                self.serialize(lhs, documents, fuzzy_scope, input);
                self.serialize(rhs, documents, fuzzy_scope, input);
            }

            Node::MatchAs(MatchAs { value, as_, .. }) => {
                self.serialize(value, documents, fuzzy_scope, input);
                self.serialize(as_, documents, fuzzy_scope, input);
            }

            Node::MatchCurrentLine(MatchCurrentLine { re, .. }) => {
                self.serialize(re, documents, fuzzy_scope, input);
            }

            // Node::MatchNilPattern(MatchNilPattern { .. }) => {}
            Node::MatchPattern(MatchPattern { value, pattern, .. }) => {
                self.serialize(value, documents, fuzzy_scope, input);
                self.serialize(pattern, documents, fuzzy_scope, input);
            }

            Node::MatchPatternP(MatchPatternP { value, pattern, .. }) => {
                self.serialize(value, documents, fuzzy_scope, input);
                self.serialize(pattern, documents, fuzzy_scope, input);
            }

            Node::MatchRest(MatchRest { name, .. }) => {
                if let Some(child_node) = name {
                    self.serialize(child_node, documents, fuzzy_scope, input);
                }
            }

            Node::MatchVar(MatchVar { name, name_l, .. }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "MatchVar",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });
            }

            Node::MatchWithLvasgn(MatchWithLvasgn { re, value, .. }) => {
                self.serialize(re, documents, fuzzy_scope, input);
                self.serialize(value, documents, fuzzy_scope, input);
            }

            Node::Mlhs(Mlhs { items, .. }) => {
                for node in items {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::Module(Module { name, body, .. }) => {
                if let Node::Const(const_node) = *name.to_owned() {
                    let node_class_scope = self.build_class_scope(&const_node);
                    let class_scope_len = node_class_scope.len();

                    for ancestor_name in node_class_scope {
                        fuzzy_scope.push(ancestor_name);
                    }

                    let (lineno, begin_pos) = input
                        .line_col_for_pos(const_node.expression_l.begin)
                        .unwrap();
                    let (_lineno, end_pos) =
                        input.line_col_for_pos(const_node.expression_l.end).unwrap();
                    let class_name = const_node.name.to_string();

                    documents.push(FuzzyNode {
                        category: "assignment",
                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                        // class_scope: node_class_scope,
                        class_scope: vec![],
                        name: class_name.clone(),
                        node_type: "Module",
                        line: lineno,
                        start_column: begin_pos,
                        end_column: end_pos,
                    });

                    fuzzy_scope.push(class_name.to_string());
                    self.class_scope.push(class_name);

                    for child_node in body {
                        self.serialize(child_node, documents, fuzzy_scope, input);
                    }

                    for _ in 0..class_scope_len {
                        fuzzy_scope.pop();
                    }

                    fuzzy_scope.pop();
                    self.class_scope.pop();
                }
            }

            Node::Next(Next { args, .. }) => {
                for node in args {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            // Node::Nil(Nil { .. }) => {}
            // Node::NthRef(NthRef { .. }) => {}
            Node::Numblock(Numblock { call, body, .. }) => {
                self.serialize(call, documents, fuzzy_scope, input);
                self.serialize(body, documents, fuzzy_scope, input);
            }

            Node::OpAsgn(OpAsgn { recv, value, .. }) => {
                self.serialize(recv, documents, fuzzy_scope, input);
                self.serialize(value, documents, fuzzy_scope, input);
            }

            Node::Optarg(Optarg {
                name,
                default,
                name_l,
                ..
            }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(name_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(name_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Optarg",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });

                self.serialize(default, documents, fuzzy_scope, input);
            }

            Node::Or(Or { lhs, rhs, .. }) => {
                self.serialize(lhs, documents, fuzzy_scope, input);
                self.serialize(rhs, documents, fuzzy_scope, input);
            }

            Node::OrAsgn(OrAsgn { recv, value, .. }) => {
                self.serialize(recv, documents, fuzzy_scope, input);
                self.serialize(value, documents, fuzzy_scope, input);
            }

            Node::Pair(Pair { key, value, .. }) => {
                self.serialize(key, documents, fuzzy_scope, input);
                self.serialize(value, documents, fuzzy_scope, input);
            }

            Node::Pin(Pin { var, .. }) => {
                self.serialize(var, documents, fuzzy_scope, input);
            }

            Node::Postexe(Postexe { body, .. }) => {
                for node in body {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::Preexe(Preexe { body, .. }) => {
                for node in body {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::Procarg0(Procarg0 { args, .. }) => {
                for node in args {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            // Node::Rational(Rational { .. }) => {}
            // Node::Redo(Redo { .. }) => {}
            Node::Regexp(Regexp { parts, options, .. }) => {
                for node in parts {
                    self.serialize(node, documents, fuzzy_scope, input);
                }

                for node in options {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            // Node::RegOpt(RegOpt { .. }) => {}
            Node::Rescue(Rescue {
                body,
                rescue_bodies,
                ..
            }) => {
                for node in body {
                    self.serialize(node, documents, fuzzy_scope, input);
                }

                for node in rescue_bodies {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::RescueBody(RescueBody {
                exc_list,
                exc_var,
                body,
                ..
            }) => {
                for node in exc_list {
                    self.serialize(node, documents, fuzzy_scope, input);
                }

                for node in exc_var {
                    self.serialize(node, documents, fuzzy_scope, input);
                }

                for node in body {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::Restarg(Restarg { name, name_l, .. }) => {
                if let Some(name_str) = name {
                    if let Some(loc) = name_l {
                        let (lineno, begin_pos) = input.line_col_for_pos(loc.begin).unwrap();
                        let (_lineno, end_pos) = input.line_col_for_pos(loc.end).unwrap();

                        documents.push(FuzzyNode {
                            category: "assignment",
                            fuzzy_ruby_scope: fuzzy_scope.clone(),
                            class_scope: vec![],
                            name: name_str.to_string(),
                            node_type: "Restarg",
                            line: lineno,
                            start_column: begin_pos,
                            end_column: end_pos,
                        });
                    }
                }
            }

            // Node::Retry(Retry { .. }) => {}
            Node::Return(Return { args, .. }) => {
                for node in args {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::SClass(SClass { expr, body, .. }) => {
                self.serialize(expr, documents, fuzzy_scope, input);

                for node in body {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            // Node::Self_(Self_ { .. }) => {}
            Node::Send(Send {
                recv,
                method_name,
                args,
                selector_l,
                ..
            }) => {
                let class_scope = if let Some(recv_node) = recv {
                    self.serialize(recv_node, documents, fuzzy_scope, input);

                    match recv_node.as_ref() {
                        Node::Const(const_node) => {
                            let mut full_class_scope = vec![const_node.name.to_string()];
                            full_class_scope.append(self.build_class_scope(&const_node).as_mut());
                            full_class_scope
                        }
                        _ => vec![],
                    }
                } else {
                    vec![]
                };

                if let Some(loc) = selector_l {
                    let (lineno, begin_pos) = input.line_col_for_pos(loc.begin).unwrap();
                    let (_lineno, end_pos) = input.line_col_for_pos(loc.end).unwrap();

                    documents.push(FuzzyNode {
                        category: "usage",
                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                        class_scope: class_scope.clone(),
                        name: method_name.to_string(),
                        node_type: "Send",
                        line: lineno,
                        start_column: begin_pos,
                        end_column: end_pos,
                    });
                }

                for node in args {
                    self.serialize(node, documents, fuzzy_scope, input);
                }

                match method_name.as_str() {
                    // Ruby
                    "attr_accessor" => {
                        for node in args {
                            match node {
                                Node::Sym(Sym {
                                    name, expression_l, ..
                                }) => {
                                    let (lineno, begin_pos) =
                                        input.line_col_for_pos(expression_l.begin).unwrap();
                                    let (_lineno, end_pos) =
                                        input.line_col_for_pos(expression_l.end).unwrap();

                                    documents.push(FuzzyNode {
                                        category: "assignment",
                                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                                        class_scope: class_scope.clone(),
                                        name: name.to_string_lossy(),
                                        node_type: "Def",
                                        line: lineno,
                                        start_column: begin_pos,
                                        end_column: end_pos,
                                    });

                                    documents.push(FuzzyNode {
                                        category: "assignment",
                                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                                        class_scope: class_scope.clone(),
                                        name: format!("{}=", name.to_string_lossy()),
                                        node_type: "Def",
                                        line: lineno,
                                        start_column: begin_pos,
                                        end_column: end_pos,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    "attr_writer" => {
                        for node in args {
                            match node {
                                Node::Sym(Sym {
                                    name, expression_l, ..
                                }) => {
                                    let (lineno, begin_pos) =
                                        input.line_col_for_pos(expression_l.begin).unwrap();
                                    let (_lineno, end_pos) =
                                        input.line_col_for_pos(expression_l.end).unwrap();

                                    documents.push(FuzzyNode {
                                        category: "assignment",
                                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                                        class_scope: class_scope.clone(),
                                        name: format!("{}=", name.to_string_lossy()),
                                        node_type: "Def",
                                        line: lineno,
                                        start_column: begin_pos,
                                        end_column: end_pos,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    "attr_reader" => {
                        for node in args {
                            match node {
                                Node::Sym(Sym {
                                    name, expression_l, ..
                                }) => {
                                    let (lineno, begin_pos) =
                                        input.line_col_for_pos(expression_l.begin).unwrap();
                                    let (_lineno, end_pos) =
                                        input.line_col_for_pos(expression_l.end).unwrap();

                                    documents.push(FuzzyNode {
                                        category: "assignment",
                                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                                        class_scope: class_scope.clone(),
                                        name: name.to_string_lossy(),
                                        node_type: "Def",
                                        line: lineno,
                                        start_column: begin_pos,
                                        end_column: end_pos,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    "alias_method" => {
                        if let Some(node) = args.first() {
                            match node {
                                Node::Sym(Sym {
                                    name, expression_l, ..
                                }) => {
                                    let (lineno, begin_pos) =
                                        input.line_col_for_pos(expression_l.begin).unwrap();
                                    let (_lineno, end_pos) =
                                        input.line_col_for_pos(expression_l.end).unwrap();

                                    documents.push(FuzzyNode {
                                        category: "assignment",
                                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                                        class_scope: class_scope.clone(),
                                        name: name.to_string_lossy(),
                                        node_type: "Def",
                                        line: lineno,
                                        start_column: begin_pos,
                                        end_column: end_pos,
                                    });
                                }
                                Node::Str(Str {
                                    value,
                                    expression_l,
                                    ..
                                }) => {
                                    let (lineno, begin_pos) =
                                        input.line_col_for_pos(expression_l.begin).unwrap();
                                    let (_lineno, end_pos) =
                                        input.line_col_for_pos(expression_l.end).unwrap();

                                    documents.push(FuzzyNode {
                                        category: "assignment",
                                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                                        class_scope: class_scope.clone(),
                                        name: value.to_string_lossy(),
                                        node_type: "Def",
                                        line: lineno,
                                        start_column: begin_pos,
                                        end_column: end_pos,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }

                    // Rails
                    "belongs_to" | "has_one" | "has_many" | "has_and_belongs_to_many" => {
                        if let Some(node) = args.first() {
                            match node {
                                Node::Sym(Sym {
                                    name, expression_l, ..
                                }) => {
                                    let (lineno, begin_pos) =
                                        input.line_col_for_pos(expression_l.begin).unwrap();
                                    let (_lineno, end_pos) =
                                        input.line_col_for_pos(expression_l.end).unwrap();

                                    documents.push(FuzzyNode {
                                        category: "assignment",
                                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                                        class_scope: class_scope.clone(),
                                        name: name.to_string_lossy(),
                                        node_type: "Def",
                                        line: lineno,
                                        start_column: begin_pos,
                                        end_column: end_pos,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {} // todo: the code below works, but it will pollute searches too
                            // much unless filtering is added when searching

                            // Rspec
                            // "let!" | "let" => {
                            //     if let Some(arg) = args.first() {
                            //         match node {
                            //             Node::Sym(Sym { name, expression_l, .. }) => {
                            //                 let (lineno, begin_pos) = input.line_col_for_pos(expression_l.begin).unwrap();
                            //                 let (_lineno, end_pos) = input.line_col_for_pos(expression_l.end).unwrap();

                            //                 documents.push(FuzzyNode {
                            //                     category: "assignment",
                            //                     fuzzy_ruby_scope: fuzzy_scope.clone(),
                            // class_scope: vec![],
                            //                     name: name.to_string_lossy(),
                            //                     node_type: "Def",
                            //                     line: lineno,
                            //                     start_column: begin_pos,
                            //                     end_column: end_pos,
                            //                 });
                            //             },
                            //             _ => {}
                            //         }
                            //     }
                            // },
                            // _ => {}
                }
            }

            Node::Shadowarg(Shadowarg { name, expression_l }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(expression_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(expression_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "assignment",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string(),
                    node_type: "Shadowarg",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });
            }

            Node::Splat(Splat { value, .. }) => {
                for node in value {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            // Node::Str(Str { .. }) => {}
            Node::Super(Super {
                args, keyword_l, ..
            }) => {
                if let Some(last_scope_name) = fuzzy_scope.last() {
                    let (lineno, begin_pos) = input.line_col_for_pos(keyword_l.begin).unwrap();
                    let (_lineno, end_pos) = input.line_col_for_pos(keyword_l.end).unwrap();

                    documents.push(FuzzyNode {
                        category: "usage",
                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                        class_scope: vec![],
                        name: last_scope_name.to_string(),
                        node_type: "Super",
                        line: lineno,
                        start_column: begin_pos,
                        end_column: end_pos,
                    });
                }

                for node in args {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::Sym(Sym {
                name, expression_l, ..
            }) => {
                let (lineno, begin_pos) = input.line_col_for_pos(expression_l.begin).unwrap();
                let (_lineno, end_pos) = input.line_col_for_pos(expression_l.end).unwrap();

                documents.push(FuzzyNode {
                    category: "usage",
                    fuzzy_ruby_scope: fuzzy_scope.clone(),
                    class_scope: vec![],
                    name: name.to_string_lossy(),
                    node_type: "Send",
                    line: lineno,
                    start_column: begin_pos,
                    end_column: end_pos,
                });
            }

            // Node::True(True { .. }) => {}
            Node::Undef(Undef { names, .. }) => {
                for node in names {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::UnlessGuard(UnlessGuard { cond, .. }) => {
                self.serialize(cond, documents, fuzzy_scope, input);
            }

            Node::Until(Until { cond, body, .. }) => {
                self.serialize(cond, documents, fuzzy_scope, input);

                for node in body {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::UntilPost(UntilPost { cond, body, .. }) => {
                self.serialize(cond, documents, fuzzy_scope, input);
                self.serialize(body, documents, fuzzy_scope, input);
            }

            Node::When(When { patterns, body, .. }) => {
                for node in patterns {
                    self.serialize(node, documents, fuzzy_scope, input);
                }

                for node in body {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::While(While { cond, body, .. }) => {
                self.serialize(cond, documents, fuzzy_scope, input);

                for node in body {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::WhilePost(WhilePost { cond, body, .. }) => {
                self.serialize(cond, documents, fuzzy_scope, input);
                self.serialize(body, documents, fuzzy_scope, input);
            }

            Node::XHeredoc(XHeredoc { parts, .. }) => {
                for node in parts {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::Xstr(Xstr { parts, .. }) => {
                for node in parts {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::Yield(Yield { args, .. }) => {
                for node in args {
                    self.serialize(node, documents, fuzzy_scope, input);
                }
            }

            Node::ZSuper(ZSuper { expression_l, .. }) => {
                if let Some(last_scope_name) = fuzzy_scope.last() {
                    let (lineno, begin_pos) = input.line_col_for_pos(expression_l.begin).unwrap();
                    let (_lineno, end_pos) = input.line_col_for_pos(expression_l.end).unwrap();

                    documents.push(FuzzyNode {
                        category: "usage",
                        fuzzy_ruby_scope: fuzzy_scope.clone(),
                        class_scope: vec![],
                        name: last_scope_name.to_string(),
                        node_type: "ZSuper",
                        line: lineno,
                        start_column: begin_pos,
                        end_column: end_pos,
                    });
                }
            }

            _ => {}
        };
    }

    fn build_class_scope(&self, const_node: &Const) -> Vec<String> {
        let mut node_class_scope = vec![];
        let mut current_node = &const_node.scope;

        loop {
            match current_node {
                Some(node) => {
                    match node.as_ref() {
                        Node::Const(Const { name, scope, .. }) => {
                            node_class_scope.push(name.to_string());
                            current_node = scope;
                        }
                        Node::Cbase(Cbase { .. }) => {
                            // let mut root_prefixed_scope = vec!["^^^".to_string()];
                            // root_prefixed_scope.append(&mut node_class_scope);

                            // node_class_scope = root_prefixed_scope;
                            break;
                        }
                        Node::Send(Send { .. }) => break,
                        Node::Self_(Self_ { expression_l: _ }) => break,
                        _ => {
                            info!("unknown node in build_class_scope");
                            info!("{:#?}", node);
                            break;
                        }
                    }
                }
                None => {
                    // node_class_scope.should = self.class_scope.clone();
                    break;
                }
            }
        }

        node_class_scope
    }
}
