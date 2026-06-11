/// dispatch.rs — Pont bytes ↔ fichiers temporaires vers les engines desktop.
/// Logique de dispatch identique à commands.rs (audit v1.7.0), sans Tauri.

use crate::conversion_engine::{
    convert_image_file, convert_svg_to_image, svg_to_dynamic_image, ImageOptions, OutputFormat,
};
use crate::office_engine::{
    csv_to_json, csv_to_txt, csv_to_xlsx, docx_to_html, docx_to_text, excel_to_csv,
    excel_to_json, excel_to_txt, pptx_to_text,
};
use crate::pdf_engine::{
    extract_text_from_pdf, get_pdf_page_count, images_to_pdf, merge_pdfs_pages,
    merge_pdfs_single_page, pdf_to_html, split_pdf,
};
use crate::text_engine::{
    create_pdf_from_text, html_to_pdf, html_to_txt, md_to_html, md_to_pdf, md_to_txt, txt_to_pdf,
};
use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicU64, Ordering};

// ─── Fichiers temporaires (RAII) ──────────────────────────────────────────────

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempFile(String);

impl TempFile {
    /// Crée un chemin temp unique (timestamp + compteur atomique — pas de collision
    /// même avec plusieurs requêtes simultanées).
    fn path(ext: &str) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir()
            .join(format!("ucw_{ts}_{n}.{ext}"))
            .to_string_lossy()
            .to_string();
        TempFile(p)
    }

    fn with_bytes(ext: &str, bytes: &[u8]) -> Result<Self> {
        let tmp = Self::path(ext);
        std::fs::write(&tmp.0, bytes).map_err(|e| anyhow!("Écriture temp : {e}"))?;
        Ok(tmp)
    }

    fn as_str(&self) -> &str {
        &self.0
    }

    fn read(&self) -> Result<Vec<u8>> {
        std::fs::read(&self.0).map_err(|e| anyhow!("Lecture résultat : {e}"))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// ─── Options ──────────────────────────────────────────────────────────────────

pub struct ConvertOptions {
    pub quality: Option<u8>,
    pub resize_width: Option<u32>,
    pub resize_height: Option<u32>,
    pub rotation: Option<u32>,
}

// ─── Conversion unifiée (match identique au desktop) ─────────────────────────

pub fn convert_bytes(input: &[u8], ext: &str, fmt: &str, opts: &ConvertOptions) -> Result<Vec<u8>> {
    let src = TempFile::with_bytes(ext, input)?;
    let dst = TempFile::path(fmt);
    let input_path = src.as_str();
    let out = dst.as_str();

    let img_opts = ImageOptions {
        quality: opts.quality,
        resize_width: opts.resize_width,
        resize_height: opts.resize_height,
        rotation: opts.rotation,
    };

    match (ext, fmt) {
        // ── Images raster → image ─────────────────────────────────────────────
        (img, f)
            if matches!(img, "png"|"jpg"|"jpeg"|"webp"|"bmp"|"gif"|"tiff"|"tif"|"tga"|"pnm"|"hdr"|"ico")
            && matches!(f, "png"|"jpg"|"jpeg"|"webp"|"bmp"|"gif"|"tiff"|"tga"|"ico") =>
        {
            let format = OutputFormat::from_str(f)?;
            convert_image_file(input_path, out, &format, &img_opts)?;
        }

        // ── Images raster → PDF ───────────────────────────────────────────────
        (img, "pdf")
            if matches!(img, "png"|"jpg"|"jpeg"|"webp"|"bmp"|"gif"|"tiff"|"tif"|"tga"|"pnm"|"hdr"|"ico") =>
        {
            images_to_pdf(&[input_path.to_string()], out)?;
        }

        // ── SVG → image raster ────────────────────────────────────────────────
        ("svg", f) if matches!(f, "png"|"jpg"|"jpeg"|"webp"|"bmp") => {
            let format = OutputFormat::from_str(f)?;
            convert_svg_to_image(input_path, out, &format, &img_opts)?;
        }

        // ── SVG → PDF ─────────────────────────────────────────────────────────
        ("svg", "pdf") => {
            let img = svg_to_dynamic_image(input_path)?;
            let tmp = TempFile::path("png");
            img.save_with_format(tmp.as_str(), image::ImageFormat::Png)?;
            images_to_pdf(&[tmp.as_str().to_string()], out)?;
        }

        // ── PDF ───────────────────────────────────────────────────────────────
        ("pdf", "txt") => {
            let text = extract_text_from_pdf(input_path)?;
            std::fs::write(out, text)?;
        }
        ("pdf", "html") => pdf_to_html(input_path, out)?,

        // ── TXT ───────────────────────────────────────────────────────────────
        ("txt", "pdf") => txt_to_pdf(input_path, out)?,

        // ── Markdown ──────────────────────────────────────────────────────────
        ("md" | "markdown", "html") => md_to_html(input_path, out)?,
        ("md" | "markdown", "txt") => md_to_txt(input_path, out)?,
        ("md" | "markdown", "pdf") => md_to_pdf(input_path, out)?,

        // ── HTML ──────────────────────────────────────────────────────────────
        ("html" | "htm", "txt") => html_to_txt(input_path, out)?,
        ("html" | "htm", "pdf") => html_to_pdf(input_path, out)?,

        // ── DOCX / DOC ────────────────────────────────────────────────────────
        ("docx" | "doc", "txt") => {
            let text = docx_to_text(input_path)?;
            std::fs::write(out, &text)?;
        }
        ("docx" | "doc", "html") => docx_to_html(input_path, out)?,
        ("docx" | "doc", "pdf") => {
            let text = docx_to_text(input_path)?;
            create_pdf_from_text(&text, out)?;
        }

        // ── PPTX / PPT ────────────────────────────────────────────────────────
        ("pptx" | "ppt", "txt") => {
            let text = pptx_to_text(input_path)?;
            std::fs::write(out, &text)?;
        }
        ("pptx" | "ppt", "pdf") => {
            let text = pptx_to_text(input_path)?;
            create_pdf_from_text(&text, out)?;
        }

        // ── Excel ─────────────────────────────────────────────────────────────
        ("xlsx" | "xls" | "ods", "csv") => excel_to_csv(input_path, out)?,
        ("xlsx" | "xls" | "ods", "json") => excel_to_json(input_path, out)?,
        ("xlsx" | "xls" | "ods", "txt") => excel_to_txt(input_path, out)?,
        ("xlsx" | "xls" | "ods", "pdf") => {
            let tmp = TempFile::path("txt");
            excel_to_txt(input_path, tmp.as_str())?;
            txt_to_pdf(tmp.as_str(), out)?;
        }

        // ── CSV ───────────────────────────────────────────────────────────────
        ("csv", "json") => csv_to_json(input_path, out)?,
        ("csv", "xlsx") => csv_to_xlsx(input_path, out)?,
        ("csv", "txt") => csv_to_txt(input_path, out)?,
        ("csv", "pdf") => {
            let tmp = TempFile::path("txt");
            csv_to_txt(input_path, tmp.as_str())?;
            txt_to_pdf(tmp.as_str(), out)?;
        }

        // ── JSON ──────────────────────────────────────────────────────────────
        ("json", "csv") => {
            let json_str = std::fs::read_to_string(input_path)?;
            json_to_csv_str(&json_str, out)?;
        }
        ("json", "txt") => {
            let raw = std::fs::read_to_string(input_path)?;
            let value: serde_json::Value = serde_json::from_str(&raw)?;
            let pretty = serde_json::to_string_pretty(&value)?;
            std::fs::write(out, pretty)?;
        }

        _ => return Err(anyhow!("Conversion .{ext} → {fmt} non supportée")),
    }

    dst.read()
}

// ─── Fusion PDF ───────────────────────────────────────────────────────────────

pub fn merge_pdfs_bytes(inputs: &[Vec<u8>], mode: &str) -> Result<Vec<u8>> {
    let temps: Vec<TempFile> = inputs
        .iter()
        .map(|b| TempFile::with_bytes("pdf", b))
        .collect::<Result<_>>()?;
    let paths: Vec<String> = temps.iter().map(|t| t.as_str().to_string()).collect();

    let dst = TempFile::path("pdf");
    match mode {
        "pages" => merge_pdfs_pages(&paths, dst.as_str())?,
        "single" => merge_pdfs_single_page(&paths, dst.as_str())?,
        _ => return Err(anyhow!("Mode inconnu : {mode}")),
    }
    dst.read()
}

// ─── Split PDF ────────────────────────────────────────────────────────────────

pub fn split_pdf_bytes(input: &[u8], pages: &[u32]) -> Result<Vec<u8>> {
    let src = TempFile::with_bytes("pdf", input)?;
    let total = get_pdf_page_count(src.as_str())?;
    for &p in pages {
        if p < 1 || p > total {
            return Err(anyhow!("Page {p} hors limites (1–{total})"));
        }
    }
    let dst = TempFile::path("pdf");
    split_pdf(src.as_str(), pages, dst.as_str())?;
    dst.read()
}

// ─── Nombre de pages PDF ──────────────────────────────────────────────────────

pub fn pdf_page_count_bytes(input: &[u8]) -> Result<u32> {
    let src = TempFile::with_bytes("pdf", input)?;
    get_pdf_page_count(src.as_str())
}

// ─── JSON → CSV (repris de commands.rs) ───────────────────────────────────────

fn json_to_csv_str(json_str: &str, output_path: &str) -> Result<()> {
    let value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| anyhow!("JSON invalide: {}", e))?;
    let records = value
        .as_array()
        .ok_or_else(|| anyhow!("JSON doit être un tableau d'objets"))?;

    if records.is_empty() {
        std::fs::write(output_path, "")?;
        return Ok(());
    }

    let headers: Vec<String> = records[0]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    let mut csv_output = headers
        .iter()
        .map(|h| crate::office_engine::csv_cell(h))
        .collect::<Vec<_>>()
        .join(",");
    csv_output.push('\n');

    for record in records {
        if let Some(obj) = record.as_object() {
            let row: Vec<String> = headers
                .iter()
                .map(|h| {
                    let v = obj
                        .get(h)
                        .map(|v| match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default();
                    crate::office_engine::csv_cell(&v)
                })
                .collect();
            csv_output.push_str(&row.join(","));
            csv_output.push('\n');
        }
    }
    std::fs::write(output_path, csv_output).map_err(|e| anyhow!("Ecriture CSV: {}", e))?;
    Ok(())
}
