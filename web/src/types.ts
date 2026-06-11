export type ConversionStatus = "idle" | "converting" | "done" | "error";
export type Rotation = 0 | 90 | 180 | 270;

export interface ImageOptions {
  quality: number;       // 1–100
  resizeWidth: string;   // "" ou nombre
  resizeHeight: string;
  rotation: Rotation;
}

export interface FileItem {
  id: string;
  name: string;
  file: File;
  extension: string;
  availableFormats: string[];
  selectedFormat: string;
  status: ConversionStatus;
  progress: number;
  /** Blob résultat (téléchargement + ZIP + fusion) */
  outputBlob?: Blob;
  /** Nom du fichier de sortie renvoyé par le serveur */
  outputName?: string;
  errorMessage?: string;
  // Métadonnées
  fileSize?: number;
  outputSize?: number;
  thumbnail?: string;
  pageCount?: number;
  // Options de conversion image
  imageOptions: ImageOptions;
  customName: string;
  showOptions: boolean;
}

export interface HistoryItem {
  id: string;
  inputName: string;
  inputExt: string;
  outputFormat: string;
  outputSize: number;
  timestamp: number;
}

export function defaultImageOptions(): ImageOptions {
  return { quality: 90, resizeWidth: "", resizeHeight: "", rotation: 0 };
}

export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

export function parsePageRange(input: string, total: number): number[] {
  const pages = new Set<number>();
  for (const part of input.split(",")) {
    const t = part.trim();
    if (!t) continue;

    if (t.includes("-")) {
      const segments = t.split("-");
      if (segments.length !== 2) throw new Error(`Plage invalide: "${t}"`);
      const a = parseInt(segments[0].trim(), 10);
      const b = parseInt(segments[1].trim(), 10);
      if (isNaN(a) || isNaN(b)) throw new Error(`Plage invalide: "${t}"`);
      if (a > b) throw new Error(`Plage inversée: "${t}" (début > fin)`);
      if (a < 1 || b > total) throw new Error(`Pages hors limites: "${t}" (1–${total})`);
      for (let i = a; i <= b; i++) pages.add(i);
    } else {
      const n = parseInt(t, 10);
      if (isNaN(n) || String(n) !== t) throw new Error(`Numéro invalide: "${t}"`);
      if (n < 1 || n > total) throw new Error(`Page ${n} hors limites (1–${total})`);
      pages.add(n);
    }
  }
  if (pages.size === 0) throw new Error("Aucune page sélectionnée");
  return Array.from(pages).sort((a, b) => a - b);
}

export const FORMAT_LABELS: Record<string, string> = {
  png: "PNG", jpg: "JPEG", webp: "WebP", bmp: "BMP", gif: "GIF",
  tiff: "TIFF", ico: "ICO", tga: "TGA", pnm: "PNM", hdr: "HDR", avif: "AVIF",
  svg: "SVG",
  pdf: "PDF", txt: "TXT", html: "HTML", md: "Markdown",
  docx: "Word", doc: "Word (Legacy)", pptx: "PowerPoint", ppt: "PowerPoint (Legacy)",
  xlsx: "Excel", xls: "Excel (Legacy)", ods: "ODS",
  csv: "CSV", json: "JSON",
};

export const IMAGE_EXTENSIONS = new Set([
  "png","jpg","jpeg","webp","bmp","gif","tiff","tif","tga","pnm","hdr","ico","svg",
]);

/** Miroir TS de get_available_formats (conversion_engine.rs) — évite un round-trip serveur. */
export function getAvailableFormats(ext: string): string[] {
  switch (ext.toLowerCase()) {
    case "png": case "jpg": case "jpeg": case "webp": case "bmp": case "gif":
    case "tiff": case "tif": case "tga": case "pnm": case "hdr": case "ico":
      return ["png", "jpg", "webp", "bmp", "gif", "tiff", "tga", "ico", "pdf"];
    case "svg": return ["png", "jpg", "webp", "bmp", "pdf"];
    case "pdf": return ["txt", "html"];
    case "txt": return ["pdf"];
    case "md": case "markdown": return ["html", "txt", "pdf"];
    case "html": case "htm": return ["txt", "pdf"];
    case "docx": case "doc": return ["txt", "html", "pdf"];
    case "pptx": case "ppt": return ["txt", "pdf"];
    case "xlsx": case "xls": case "ods": return ["csv", "json", "txt", "pdf"];
    case "csv": return ["json", "xlsx", "txt", "pdf"];
    case "json": return ["csv", "txt"];
    default: return [];
  }
}
