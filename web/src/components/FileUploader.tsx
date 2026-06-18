import { useRef, useState } from "react";
import { FileItem, IMAGE_EXTENSIONS, defaultImageOptions, getAvailableFormats } from "../types";
import { makeThumbnail, pdfPageCount } from "../api";

const MEMORY_KEY = "uc_default_formats";

function getDefaultFormats(): Record<string, string> {
  try { return JSON.parse(localStorage.getItem(MEMORY_KEY) ?? "{}"); } catch { return {}; }
}
export function saveDefaultFormat(ext: string, fmt: string) {
  const mem = getDefaultFormats();
  mem[ext] = fmt;
  localStorage.setItem(MEMORY_KEY, JSON.stringify(mem));
}

const ACCEPTED = [
  "png","jpg","jpeg","webp","bmp","gif","tiff","tif","tga","pnm","hdr","ico","svg",
  "pdf","txt","md","markdown","html","htm",
  "docx","doc","pptx","ppt",
  "xlsx","xls","ods","csv","json",
];
const ACCEPT_ATTR = ACCEPTED.map((e) => `.${e}`).join(",");

interface Props {
  onFilesAdded: (files: FileItem[]) => void;
}

const MAX_FILE_BYTES = 60 * 1024 * 1024; // 60 MB — cohérent avec DefaultBodyLimit serveur

export function FileUploader({ onFilesAdded }: Props) {
  const [isDragging, setIsDragging] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  async function handleFiles(list: FileList | null) {
    if (!list || list.length === 0) return;
    const memory = getDefaultFormats();

    const items: FileItem[] = await Promise.all(
      Array.from(list)
        .filter((f) => f.size <= MAX_FILE_BYTES) // Rejet préemptif côté client
        .map(async (f) => {
        const name = f.name;
        const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
        const formats = getAvailableFormats(ext);

        // Format par défaut : mémorisé > premier format différent de l'entrée
        const memorized = memory[ext];
        const defaultFmt = (memorized && memorized !== ext && formats.includes(memorized))
          ? memorized
          : (formats.find((fm) => fm !== ext) ?? formats[0] ?? "");

        // Miniature (images seulement, décodées par le navigateur)
        let thumbnail: string | undefined;
        if (IMAGE_EXTENSIONS.has(ext)) {
          thumbnail = await makeThumbnail(f);
        }

        // Nombre de pages (PDF)
        let pageCount: number | undefined;
        if (ext === "pdf") {
          try { pageCount = await pdfPageCount(f); } catch { }
        }

        return {
          id: crypto.randomUUID(),
          name, file: f, extension: ext,
          availableFormats: formats,
          selectedFormat: defaultFmt,
          status: "idle" as const,
          progress: 0,
          fileSize: f.size, thumbnail, pageCount,
          imageOptions: defaultImageOptions(),
          customName: "",
          showOptions: false,
        };
      })
    );
    onFilesAdded(items.filter((f) => f.availableFormats.length > 0));
  }

  return (
    <div
      className={`
        flex flex-col items-center justify-center
        w-full h-48 rounded-2xl border-2 border-dashed
        transition-all duration-200 cursor-pointer select-none
        ${isDragging
          ? "border-blue-400 bg-blue-950/40 scale-[1.01]"
          : "border-slate-600 bg-slate-800/40 hover:border-slate-400"
        }
      `}
      onClick={() => inputRef.current?.click()}
      onDragOver={(e) => { e.preventDefault(); setIsDragging(true); }}
      onDragEnter={(e) => { e.preventDefault(); setIsDragging(true); }}
      onDragLeave={(e) => { e.preventDefault(); setIsDragging(false); }}
      onDrop={(e) => {
        e.preventDefault();
        setIsDragging(false);
        void handleFiles(e.dataTransfer.files);
      }}
    >
      <input
        ref={inputRef}
        type="file"
        multiple
        accept={ACCEPT_ATTR}
        className="hidden"
        onChange={(e) => {
          void handleFiles(e.target.files);
          e.target.value = "";
        }}
      />
      <svg className="w-10 h-10 mb-2 text-slate-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5}
          d="M12 16v-8m0 0-3 3m3-3 3 3M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1" />
      </svg>
      <p className="text-slate-300 font-medium">
        {isDragging ? "Relâchez les fichiers ici" : "Cliquez ou déposez vos fichiers"}
      </p>
      <p className="text-slate-500 text-sm mt-1">
        Images · PDF · Word · Excel · PowerPoint · CSV · JSON
      </p>
    </div>
  );
}
