/// main.rs — UniversalConverter Web : API axum + frontend statique.
/// Réutilise les engines Rust du desktop (audit v1.7.0) via fichiers temporaires.

mod conversion_engine;
mod dispatch;
mod office_engine;
mod pdf_engine;
mod text_engine;

use anyhow::anyhow;
use axum::{
    extract::{DefaultBodyLimit, Multipart, State},
    http::{header, HeaderName, HeaderValue, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Semaphore;
use tower_governor::{
    governor::GovernorConfigBuilder, key_extractor::KeyExtractor, GovernorError, GovernorLayer,
};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    timeout::TimeoutLayer,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_UPLOAD_BYTES: usize = 60 * 1024 * 1024; // 60 MB (multipart total)
const MAX_INFLIGHT: usize = 2;

struct AppState {
    conversion_permits: Semaphore,
}

// ─── Extraction IP cliente (derrière nginx) ───────────────────────────────────

#[derive(Clone)]
struct ClientIpExtractor;

impl KeyExtractor for ClientIpExtractor {
    type Key = IpAddr;

    fn extract<B>(&self, req: &Request<B>) -> Result<Self::Key, GovernorError> {
        let header_ip = |name: &str| -> Option<IpAddr> {
            req.headers()
                .get(name)?
                .to_str()
                .ok()?
                .split(',')
                .next()?
                .trim()
                .parse()
                .ok()
        };

        header_ip("cf-connecting-ip")
            .or_else(|| header_ip("x-real-ip"))
            .or_else(|| {
                req.extensions()
                    .get::<axum::extract::ConnectInfo<SocketAddr>>()
                    .map(|ci| ci.0.ip())
            })
            .ok_or(GovernorError::UnableToExtractKey)
    }
}

// ─── Erreur API ───────────────────────────────────────────────────────────────

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

fn bad_request(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

fn internal(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, msg.into())
}

// ─── Helpers multipart ────────────────────────────────────────────────────────

struct UploadedFile {
    name: String,
    ext: String,
    bytes: Vec<u8>,
}

/// Extension validée : alphanumérique, 10 chars max (anti injection chemin).
fn safe_ext(filename: &str) -> String {
    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase();
    if !ext.is_empty()
        && ext.len() <= 10
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext != filename.to_lowercase()
    {
        ext
    } else {
        String::new()
    }
}

struct ParsedRequest {
    files: Vec<UploadedFile>,
    fields: std::collections::HashMap<String, String>,
}

async fn parse_multipart(mut multipart: Multipart) -> Result<ParsedRequest, ApiError> {
    let mut files = Vec::new();
    let mut fields = std::collections::HashMap::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(format!("Multipart invalide : {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" || name == "files" {
            let filename = field.file_name().unwrap_or("input").to_string();
            let ext = safe_ext(&filename);
            let bytes = field
                .bytes()
                .await
                .map_err(|e| bad_request(format!("Lecture fichier : {e}")))?
                .to_vec();
            files.push(UploadedFile { name: filename, ext, bytes });
        } else {
            let value = field
                .text()
                .await
                .map_err(|e| bad_request(format!("Lecture champ '{name}' : {e}")))?;
            fields.insert(name, value);
        }
    }

    Ok(ParsedRequest { files, fields })
}

fn mime_for(fmt: &str) -> &'static str {
    match fmt {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "tiff" => "image/tiff",
        "ico" => "image/x-icon",
        "tga" => "application/octet-stream",
        "pdf" => "application/pdf",
        "txt" => "text/plain; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "json" => "application/json",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

static X_OUTPUT_NAME: HeaderName = HeaderName::from_static("x-output-name");

fn file_response(fmt: &str, output_name: &str, bytes: Vec<u8>) -> Response {
    // Nom ASCII-safe pour le header (le frontend gère le nom complet)
    let safe_name: String = output_name
        .chars()
        .map(|c| if c.is_ascii_graphic() && c != '"' { c } else { '_' })
        .collect();
    let content_disposition = format!("attachment; filename=\"{}\"", safe_name);
    (
        [
            (header::CONTENT_TYPE.clone(), mime_for(fmt).to_string()),
            (header::CACHE_CONTROL.clone(), "no-store".to_string()),
            (header::CONTENT_DISPOSITION.clone(), content_disposition),
            (X_OUTPUT_NAME.clone(), safe_name),
        ],
        bytes,
    )
        .into_response()
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "version": VERSION }))
}

/// POST /api/convert — multipart : file + output_format [+ quality, resize_width,
/// resize_height, rotation, output_name] → fichier converti (binaire).
async fn convert(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let req = parse_multipart(multipart).await?;

    let file = req.files.into_iter().next().ok_or_else(|| bad_request("Champ 'file' manquant"))?;
    if file.bytes.is_empty() {
        return Err(bad_request("Fichier vide"));
    }
    if file.ext.is_empty() {
        return Err(bad_request("Extension de fichier introuvable ou invalide"));
    }

    let fmt = req
        .fields
        .get("output_format")
        .map(|f| f.to_lowercase())
        .ok_or_else(|| bad_request("Champ 'output_format' manquant"))?;
    if fmt.len() > 10 || !fmt.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(bad_request("Format de sortie invalide"));
    }

    let parse_u32 = |key: &str| -> Option<u32> { req.fields.get(key).and_then(|v| v.parse().ok()) };
    let opts = dispatch::ConvertOptions {
        quality: req.fields.get("quality").and_then(|v| v.parse().ok()),
        resize_width: parse_u32("resize_width"),
        resize_height: parse_u32("resize_height"),
        rotation: parse_u32("rotation"),
    };

    let stem = std::path::Path::new(&file.name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output")
        .to_string();
    let output_name = match req.fields.get("output_name").map(|s| s.trim()) {
        Some(n) if !n.is_empty() => format!("{n}.{fmt}"),
        _ => format!("{stem}_converted.{fmt}"),
    };

    let _permit = state
        .conversion_permits
        .acquire()
        .await
        .map_err(|_| internal("Serveur en cours d'arrêt"))?;

    let fmt_clone = fmt.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        dispatch::convert_bytes(&file.bytes, &file.ext, &fmt_clone, &opts)
    })
    .await
    .map_err(|e| internal(format!("Tâche interrompue : {e}")))?
    .map_err(|e| {
        tracing::error!("Conversion échouée : {e}");
        // Filtre les chemins de fichiers temporaires des messages d'erreur exposés
        let msg = sanitize_error_message(&e.to_string());
        bad_request(msg)
    })?;

    Ok(file_response(&fmt, &output_name, bytes))
}

/// POST /api/merge-pdf — multipart : files (2+) + mode (pages|single) → PDF.
async fn merge_pdf(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let req = parse_multipart(multipart).await?;

    if req.files.len() < 2 {
        return Err(bad_request("Au moins 2 fichiers PDF requis"));
    }
    let mode = req.fields.get("mode").map(String::as_str).unwrap_or("pages");
    if mode != "pages" && mode != "single" {
        return Err(bad_request(format!("Mode inconnu : {mode}")));
    }

    let _permit = state
        .conversion_permits
        .acquire()
        .await
        .map_err(|_| internal("Serveur en cours d'arrêt"))?;

    let mode = mode.to_string();
    let inputs: Vec<Vec<u8>> = req.files.into_iter().map(|f| f.bytes).collect();
    let bytes = tokio::task::spawn_blocking(move || dispatch::merge_pdfs_bytes(&inputs, &mode))
        .await
        .map_err(|e| internal(format!("Tâche interrompue : {e}")))?
        .map_err(|e| {
            tracing::error!("Fusion PDF échouée : {e}");
            bad_request(sanitize_error_message(&e.to_string()))
        })?;

    Ok(file_response("pdf", "merged.pdf", bytes))
}

/// POST /api/split-pdf — multipart : file + pages ("1,3,5") → PDF.
async fn split_pdf(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let req = parse_multipart(multipart).await?;

    let file = req.files.into_iter().next().ok_or_else(|| bad_request("Champ 'file' manquant"))?;
    let pages: Vec<u32> = req
        .fields
        .get("pages")
        .ok_or_else(|| bad_request("Champ 'pages' manquant"))?
        .split(',')
        .map(|p| p.trim().parse::<u32>())
        .collect::<Result<_, _>>()
        .map_err(|_| bad_request("Liste de pages invalide (attendu : 1,3,5)"))?;
    if pages.is_empty() {
        return Err(bad_request("Aucune page sélectionnée"));
    }

    let stem = std::path::Path::new(&file.name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document")
        .to_string();

    let _permit = state
        .conversion_permits
        .acquire()
        .await
        .map_err(|_| internal("Serveur en cours d'arrêt"))?;

    let bytes = tokio::task::spawn_blocking(move || dispatch::split_pdf_bytes(&file.bytes, &pages))
        .await
        .map_err(|e| internal(format!("Tâche interrompue : {e}")))?
        .map_err(|e| {
            tracing::error!("Split PDF échoué : {e}");
            bad_request(sanitize_error_message(&e.to_string()))
        })?;

    Ok(file_response("pdf", &format!("{stem}_pages.pdf"), bytes))
}

/// POST /api/pdf-page-count — multipart : file → {"pages": n}.
async fn pdf_page_count(multipart: Multipart) -> Result<Response, ApiError> {
    let req = parse_multipart(multipart).await?;
    let file = req.files.into_iter().next().ok_or_else(|| bad_request("Champ 'file' manquant"))?;

    // Limite spécifique à cet endpoint : 10 MB
    const MAX_PDF_COUNT_BYTES: usize = 10 * 1024 * 1024;
    if file.bytes.len() > MAX_PDF_COUNT_BYTES {
        return Err(bad_request("Fichier trop volumineux pour ce endpoint (max 10 MB)"));
    }

    // Validation magic bytes PDF (%PDF)
    if !file.bytes.starts_with(b"%PDF") {
        return Err(bad_request("Fichier invalide : signature PDF (%PDF) introuvable"));
    }

    let count = tokio::task::spawn_blocking(move || dispatch::pdf_page_count_bytes(&file.bytes))
        .await
        .map_err(|e| internal(format!("Tâche interrompue : {e}")))?
        .map_err(|e| bad_request(sanitize_error_message(&e.to_string())))?;

    Ok(Json(serde_json::json!({ "pages": count })).into_response())
}

// ─── Filtrage des messages d'erreur exposés au client ────────────────────────

/// Supprime les chemins de fichiers temporaires et détails système des messages
/// d'erreur avant de les renvoyer au client. Les chemins comme `/tmp/ucw_*.xxx`
/// ou `C:\Users\...\AppData\Local\Temp\ucw_*` ne doivent pas fuiter.
fn sanitize_error_message(msg: &str) -> String {
    // Supprimer les chemins absolus (Windows et POSIX)
    let re_win = regex::Regex::new(r"(?i)[A-Za-z]:[\\\/][^\s,:'\"]+").unwrap();
    let re_posix = regex::Regex::new(r"/(?:tmp|var|home|usr|opt)/[^\s,:'\"]+").unwrap();
    let cleaned = re_win.replace_all(msg, "<path>");
    let cleaned = re_posix.replace_all(&cleaned, "<path>");
    cleaned.into_owned()
}

// ─── main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3003);
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "../web/dist".into());
    let allowed_origin = std::env::var("ALLOWED_ORIGIN")
        .unwrap_or_else(|_| "https://universalconverter-app.heiphaistos.org".into());

    let state = Arc::new(AppState {
        conversion_permits: Semaphore::new(MAX_INFLIGHT),
    });

    // Rate-limit : burst 20, recharge 1 toutes les 3 s (≈ 20 req/min/IP)
    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(3)
            .burst_size(20)
            .key_extractor(ClientIpExtractor)
            .finish()
            .ok_or_else(|| anyhow!("Config rate-limit invalide"))?,
    );

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(
            allowed_origin.parse::<HeaderValue>()
                .map_err(|e| anyhow!("ALLOWED_ORIGIN invalide : {e}"))?,
        ));

    // Security headers appliqués à toutes les réponses
    let csp = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; frame-ancestors 'none'";
    let sec_headers = tower::ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(csp),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ));

    let api = Router::new()
        .route("/api/convert", post(convert))
        .route("/api/merge-pdf", post(merge_pdf))
        .route("/api/split-pdf", post(split_pdf))
        .route("/api/pdf-page-count", post(pdf_page_count))
        .route("/api/health", get(health))
        .layer(cors)
        .layer(GovernorLayer { config: governor_conf })
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
        .layer(TimeoutLayer::new(Duration::from_secs(120)))
        .with_state(state);

    let index = format!("{static_dir}/index.html");
    let static_service = ServeDir::new(&static_dir).fallback(ServeFile::new(&index));

    let app = api.fallback_service(static_service).layer(sec_headers);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("UniversalConverter Web v{VERSION} — écoute sur http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Arrêt demandé");
    })
    .await?;

    Ok(())
}
