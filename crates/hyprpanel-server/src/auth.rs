//! Authentification minimaliste : jeton statique, comparaison en temps
//! constant, passé soit en en-tête `Authorization: Bearer <jeton>`, soit en
//! query `?key=<jeton>` (clients MCP sans support des en-têtes).

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header::AUTHORIZATION, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Comparaison en temps constant (les longueurs divergent → faux, mais sans
/// court-circuit sur les octets comparés).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

fn bearer_token(req: &Request) -> Option<&str> {
    req.headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

fn query_key(req: &Request) -> Option<&str> {
    let q = req.uri().query()?;
    q.split('&')
        .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
        .find(|(k, _)| *k == "key")
        .map(|(_, v)| v)
        .filter(|v| !v.is_empty())
}

pub async fn middleware(State(token): State<Arc<String>>, req: Request, next: Next) -> Response {
    let ok = [bearer_token(&req), query_key(&req)]
        .into_iter()
        .flatten()
        .any(|candidate| constant_time_eq(candidate.as_bytes(), token.as_bytes()));
    if ok {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, "jeton manquant ou invalide").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparaison_temps_constant() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        // deux chaînes vides sont « égales » : le middleware filtre les
        // candidats vides avant d'arriver ici.
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn cle_query_extraite() {
        let req = Request::builder()
            .uri("/mcp?key=abc123&autre=1")
            .body(axum::body::Body::empty()).unwrap();
        assert_eq!(query_key(&req), Some("abc123"));

        let req = Request::builder().uri("/mcp").body(axum::body::Body::empty()).unwrap();
        assert_eq!(query_key(&req), None);
    }
}
