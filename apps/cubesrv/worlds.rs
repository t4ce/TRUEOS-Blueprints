//! Immutable Key5 exports, separate from Key8's frozen demo world.
use axum::{
    Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
pub fn router(catalog: &'static [&'static [u8]]) -> Router {
    Router::new()
        .route("/worlds/{id}", get(world))
        .with_state(catalog)
}
async fn world(
    State(catalog): State<&'static [&'static [u8]]>,
    Path(id): Path<usize>,
) -> impl IntoResponse {
    match id.checked_sub(1).and_then(|i| catalog.get(i)) {
        Some(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            *bytes,
        ),
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            b"unknown world".as_slice(),
        ),
    }
}
