use candid_language_server::CandidLanguageServer;
use tower_lsp_server::{LspService, Server};

#[tokio::main]
async fn main() {
    env_logger::init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(CandidLanguageServer::new).finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}
