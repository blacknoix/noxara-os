pub mod ask;
pub mod chat;
pub mod common;
pub mod documents;
pub mod insights;
pub mod proposals;
pub mod sessions;
pub mod settings;
pub mod suggestions;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(chat::router())
        .merge(sessions::router())
        .merge(settings::router())
        .merge(insights::router())
        .merge(proposals::router())
        .merge(ask::router())
        .merge(documents::router())
        .merge(suggestions::router())
}
