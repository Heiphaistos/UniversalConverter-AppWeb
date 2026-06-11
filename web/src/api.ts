import type { ImageOptions } from "./types";

export interface ConvertResponse {
  blob: Blob;
  outputName: string;
}

async function throwApiError(res: Response): Promise<never> {
  if (res.status === 429) {
    throw new Error("Trop de requêtes — patientez une minute avant de réessayer.");
  }
  const msg = await res
    .json()
    .then((j: { error?: string }) => j.error)
    .catch(() => undefined);
  throw new Error(msg ?? `Erreur serveur (${res.status})`);
}

function outputNameFrom(res: Response, fallback: string): string {
  return res.headers.get("X-Output-Name") ?? fallback;
}

/** Conversion d'un fichier. */
export async function convertFile(
  file: File,
  outputFormat: string,
  options: { imageOptions?: ImageOptions; outputName?: string } = {}
): Promise<ConvertResponse> {
  const form = new FormData();
  form.append("file", file);
  form.append("output_format", outputFormat);

  const img = options.imageOptions;
  if (img) {
    if (["jpg", "jpeg"].includes(outputFormat)) form.append("quality", String(img.quality));
    if (img.resizeWidth) form.append("resize_width", img.resizeWidth);
    if (img.resizeHeight) form.append("resize_height", img.resizeHeight);
    if (img.rotation !== 0) form.append("rotation", String(img.rotation));
  }
  if (options.outputName?.trim()) form.append("output_name", options.outputName.trim());

  const res = await fetch("/api/convert", { method: "POST", body: form });
  if (!res.ok) await throwApiError(res);

  return {
    blob: await res.blob(),
    outputName: outputNameFrom(res, `${file.name.replace(/\.[^.]+$/, "")}_converted.${outputFormat}`),
  };
}

/** Fusion de PDFs (File ou Blob). */
export async function mergePdfs(
  files: (File | Blob)[],
  mode: "pages" | "single",
  outputName: string
): Promise<ConvertResponse> {
  const form = new FormData();
  files.forEach((f) => form.append("files", f));
  form.append("mode", mode);

  const res = await fetch("/api/merge-pdf", { method: "POST", body: form });
  if (!res.ok) await throwApiError(res);

  return { blob: await res.blob(), outputName: `${outputName}.pdf` };
}

/** Extraction de pages d'un PDF. */
export async function splitPdf(file: File, pages: number[]): Promise<ConvertResponse> {
  const form = new FormData();
  form.append("file", file);
  form.append("pages", pages.join(","));

  const res = await fetch("/api/split-pdf", { method: "POST", body: form });
  if (!res.ok) await throwApiError(res);

  return {
    blob: await res.blob(),
    outputName: outputNameFrom(res, `${file.name.replace(/\.[^.]+$/, "")}_pages.pdf`),
  };
}

/** Nombre de pages d'un PDF. */
export async function pdfPageCount(file: File): Promise<number> {
  const form = new FormData();
  form.append("file", file);

  const res = await fetch("/api/pdf-page-count", { method: "POST", body: form });
  if (!res.ok) await throwApiError(res);

  const data = (await res.json()) as { pages: number };
  return data.pages;
}

export async function checkHealth(): Promise<{ status: string; version: string }> {
  const res = await fetch("/api/health");
  if (!res.ok) throw new Error(`Serveur indisponible (${res.status})`);
  return res.json();
}

/** Téléchargement d'un blob côté navigateur. */
export function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

/** Miniature client-side (images raster + SVG via le navigateur). */
export function makeThumbnail(file: File): Promise<string | undefined> {
  return new Promise((resolve) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      const scale = Math.min(120 / img.width, 120 / img.height, 1);
      const canvas = document.createElement("canvas");
      canvas.width = Math.max(1, Math.round(img.width * scale));
      canvas.height = Math.max(1, Math.round(img.height * scale));
      canvas.getContext("2d")?.drawImage(img, 0, 0, canvas.width, canvas.height);
      URL.revokeObjectURL(url);
      try {
        resolve(canvas.toDataURL("image/png"));
      } catch {
        resolve(undefined);
      }
    };
    img.onerror = () => {
      URL.revokeObjectURL(url);
      resolve(undefined); // format non décodable par le navigateur (TIFF, TGA…)
    };
    img.src = url;
  });
}
