use candid_language_server::CandidLanguageServer;
use tower_lsp_server::{LspService, Server};
#[cfg(feature = "tracing")]
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() {
    init_tracing();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(CandidLanguageServer::new).finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}

fn init_tracing() {
    #[cfg(feature = "tracing")]
    {
        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        fmt().with_env_filter(env_filter).init();
    }
}
