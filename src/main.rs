use candid_language_server::lsp::CandidLanguageServer;
use dashmap::DashMap;
use rapidhash::fast::RandomState;
use tower_lsp_server::{LspService, Server};

#[tokio::main]
async fn main() {
    env_logger::init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let random_state = RandomState::new();

    let (service, socket) = LspService::build(|client| CandidLanguageServer {
        client,
        ast_map: DashMap::with_hasher(random_state),
        document_map: DashMap::with_hasher(random_state),
        semantic_token_map: DashMap::with_hasher(random_state),
        semantic_map: DashMap::with_hasher(random_state),
    })
    .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}
