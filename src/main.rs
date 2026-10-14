use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;
use tokio::sync::RwLock;
use traest::{InvokeError, Process, Registry};

#[derive(Clone)]
struct AppState {
    registry: Arc<RwLock<Registry>>,
}

#[tokio::main]
async fn main() {
    let registry = Arc::new(RwLock::new(Registry::default()));
    let state = AppState { registry };
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/functions", get(get_functions))
        .route("/functions", post(create_function))
        .route("/invoke/{name}", post(invoke_function))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("localhost:8080")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn get_functions(State(state): State<AppState>) -> Json<Vec<String>> {
    let locked_registry = state.registry.read().await;
    let results = locked_registry.list();
    drop(locked_registry);

    Json(results)
}

#[derive(Debug, Deserialize)]
struct CreateCommand {
    command: String,
    args: Vec<String>,
    name: String,
}
async fn create_function(
    State(state): State<AppState>,
    Json(body): Json<CreateCommand>,
) -> Result<StatusCode, InvokeError> {
    let mut locked_registry = state.registry.write().await;
    let new_function = Process {
        command: body.command,
        args: body.args,
    };
    locked_registry.register(body.name, new_function)?;
    Ok(StatusCode::CREATED)
}

async fn invoke_function(
    State(state): State<AppState>,
    Path(name): Path<String>,
    body: Bytes,
) -> Result<Vec<u8>, InvokeError> {
    let store = state.registry.clone();

    let work = tokio::task::spawn_blocking(move || {
        let registry = store.blocking_read();
        registry.invoke_by_name(&name, &body)
    });

    match tokio::time::timeout(Duration::from_secs(2), work).await {
        Err(_) => Err(InvokeError::Timeout),
        Ok(Err(_)) => Err(InvokeError::InternalError),
        Ok(Ok(result)) => result,
    }
}
