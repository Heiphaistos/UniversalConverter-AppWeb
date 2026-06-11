import { useEffect, useState } from "react";
import JSZip from "jszip";
import { FileUploader } from "./components/FileUploader";
import { FileList } from "./components/FileList";
import { History } from "./components/History";
import { MergePDF } from "./components/MergePDF";
import { MergePromptModal } from "./components/MergePromptModal";
import { convertFile, checkHealth, downloadBlob } from "./api";
import { FileItem, HistoryItem } from "./types";

declare const __APP_VERSION__: string;

const HISTORY_KEY = "uc_history";
const MAX_HISTORY = 50;

function loadHistory(): HistoryItem[] {
  try { return JSON.parse(localStorage.getItem(HISTORY_KEY) ?? "[]"); } catch { return []; }
}
function saveHistory(items: HistoryItem[]) {
  localStorage.setItem(HISTORY_KEY, JSON.stringify(items.slice(0, MAX_HISTORY)));
}

export default function App() {
  const [files, setFiles] = useState<FileItem[]>([]);
  const [history, setHistory] = useState<HistoryItem[]>(loadHistory);
  const [showHistory, setShowHistory] = useState(false);
  const [showMergePDF, setShowMergePDF] = useState(false);
  const [mergePromptIds, setMergePromptIds] = useState<string[] | null>(null);
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const [serverDown, setServerDown] = useState(false);

  useEffect(() => {
    checkHealth().catch(() => setServerDown(true));
  }, []);

  function showToast(msg: string, ok: boolean) {
    setToast({ msg, ok });
    setTimeout(() => setToast(null), 4000);
  }

  // ── Gestion des fichiers ────────────────────────────────────────────────────

  function addFiles(newFiles: FileItem[]) {
    setFiles((prev) => {
      const existing = new Set(prev.map((f) => `${f.name}:${f.fileSize}`));
      return [...prev, ...newFiles.filter((f) => !existing.has(`${f.name}:${f.fileSize}`))];
    });
  }

  function updateFile(id: string, patch: Partial<FileItem>) {
    setFiles((prev) => prev.map((f) => (f.id === id ? { ...f, ...patch } : f)));
  }

  function removeFile(id: string) {
    setFiles((prev) => prev.filter((f) => f.id !== id));
  }

  function clearAll() { setFiles([]); }

  // ── Conversion ─────────────────────────────────────────────────────────────

  async function runConversion(file: FileItem) {
    if (!file.selectedFormat || file.status !== "idle") return;
    updateFile(file.id, { status: "converting", progress: 10 });

    const timer = setInterval(() => {
      setFiles((prev) => prev.map((f) =>
        f.id === file.id && f.status === "converting"
          ? { ...f, progress: Math.min(f.progress + 12, 88) }
          : f
      ));
    }, 200);

    try {
      const result = await convertFile(file.file, file.selectedFormat, {
        imageOptions: file.imageOptions,
        outputName: file.customName,
      });
      clearInterval(timer);
      updateFile(file.id, {
        status: "done", progress: 100,
        outputBlob: result.blob,
        outputName: result.outputName,
        outputSize: result.blob.size,
      });
      // Historique
      const item: HistoryItem = {
        id: crypto.randomUUID(),
        inputName: file.name,
        inputExt: file.extension,
        outputFormat: file.selectedFormat,
        outputSize: result.blob.size,
        timestamp: Date.now(),
      };
      setHistory((prev) => {
        const next = [item, ...prev].slice(0, MAX_HISTORY);
        saveHistory(next);
        return next;
      });
    } catch (err: unknown) {
      clearInterval(timer);
      const msg = typeof err === "string" ? err
        : err instanceof Error ? err.message
        : JSON.stringify(err);
      updateFile(file.id, { status: "error", progress: 0, errorMessage: msg });
    }
  }

  async function convertAll() {
    const idle = files.filter((f) => f.status === "idle" && f.selectedFormat);
    // IDs des fichiers ciblant PDF (pour le prompt de fusion post-conversion)
    const pdfTargetIds = idle
      .filter((f) => f.selectedFormat === "pdf")
      .map((f) => f.id);

    // Séquentiel : le serveur sérialise l'inférence de toute façon (semaphore)
    for (const f of idle) {
      await runConversion(f);
    }

    if (pdfTargetIds.length >= 2) {
      setMergePromptIds(pdfTargetIds);
    }
  }

  // ── ZIP des fichiers convertis ─────────────────────────────────────────────

  async function zipConverted() {
    const done = files.filter((f) => f.status === "done" && f.outputBlob);
    if (done.length === 0) return;
    try {
      const zip = new JSZip();
      const used = new Set<string>();
      for (const f of done) {
        let name = f.outputName ?? "output";
        let counter = 2;
        while (used.has(name)) {
          const dot = name.lastIndexOf(".");
          name = dot > 0
            ? `${name.slice(0, dot)}_${counter}${name.slice(dot)}`
            : `${name}_${counter}`;
          counter++;
        }
        used.add(name);
        zip.file(name, f.outputBlob!);
      }
      const blob = await zip.generateAsync({ type: "blob" });
      downloadBlob(blob, "converted_files.zip");
      showToast("ZIP téléchargé", true);
    } catch (e: unknown) {
      showToast(`Erreur ZIP : ${e instanceof Error ? e.message : String(e)}`, false);
    }
  }

  // ── Historique ─────────────────────────────────────────────────────────────

  function clearHistory() {
    setHistory([]);
    localStorage.removeItem(HISTORY_KEY);
  }

  // ── Compteurs ──────────────────────────────────────────────────────────────

  const idleCount = files.filter((f) => f.status === "idle").length;
  const doneCount = files.filter((f) => f.status === "done").length;

  // ── Rendu ──────────────────────────────────────────────────────────────────

  return (
    <div className="min-h-screen bg-slate-900 text-white flex flex-col">

      {/* Header */}
      <header className="border-b border-slate-800 px-5 py-3 flex items-center justify-between shrink-0 gap-2">
        <div className="flex items-center gap-3">
          <div className="w-7 h-7 bg-blue-600 rounded-lg flex items-center justify-center">
            <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2}
                d="M8 7h12m0 0-4-4m4 4-4 4m0 6H4m0 0 4 4m-4-4 4-4" />
            </svg>
          </div>
          <h1 className="text-base font-bold">Universal Converter</h1>
          <span className="text-xs text-slate-500 bg-slate-800 rounded px-2 py-0.5">v{__APP_VERSION__}</span>
        </div>

        <div className="flex items-center gap-1.5 flex-wrap justify-end">
          {/* Fusionner PDFs */}
          <button onClick={() => setShowMergePDF(true)}
            className="text-xs text-slate-400 hover:text-slate-200 bg-slate-800 hover:bg-slate-700 px-2.5 py-1.5 rounded-lg transition-colors border border-slate-700">
            Fusionner PDFs
          </button>

          {/* ZIP */}
          {doneCount > 0 && (
            <button onClick={zipConverted}
              className="text-xs text-slate-400 hover:text-slate-200 bg-slate-800 hover:bg-slate-700 px-2.5 py-1.5 rounded-lg transition-colors border border-slate-700">
              ZIP ({doneCount})
            </button>
          )}

          {/* Tout convertir */}
          {idleCount > 0 && files.length > 0 && (
            <button onClick={convertAll}
              className="bg-blue-600 hover:bg-blue-500 text-white text-xs px-3 py-1.5 rounded-lg font-medium transition-colors">
              Tout convertir ({idleCount})
            </button>
          )}

          {/* Vider */}
          {files.length > 0 && (
            <button onClick={clearAll}
              className="text-slate-400 hover:text-red-400 text-xs px-2.5 py-1.5 rounded-lg transition-colors">
              Vider
            </button>
          )}

          {/* Historique */}
          <button onClick={() => setShowHistory(true)}
            className="relative text-slate-400 hover:text-slate-200 p-1.5 rounded-lg hover:bg-slate-800 transition-colors">
            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2}
                d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            {history.length > 0 && (
              <span className="absolute -top-1 -right-1 w-4 h-4 bg-blue-600 rounded-full text-[10px] flex items-center justify-center">
                {history.length > 9 ? "9+" : history.length}
              </span>
            )}
          </button>
        </div>
      </header>

      {/* Bandeau serveur down */}
      {serverDown && (
        <div className="mx-5 mt-4 p-3 rounded-xl bg-red-950/60 border border-red-900 text-red-300 text-sm">
          Service indisponible — réessayez dans quelques instants.
        </div>
      )}

      {/* Contenu */}
      <main className="flex-1 max-w-3xl w-full mx-auto px-5 py-6">
        <FileUploader onFilesAdded={addFiles} />
        <FileList files={files} onUpdate={updateFile} onRemove={removeFile} onConvert={runConversion} />

        {files.length === 0 && (
          <div className="mt-8 text-center text-slate-600 text-sm space-y-1">
            <p className="font-medium text-slate-500">Formats supportés</p>
            <p>Images → PNG · JPG · WebP · BMP · GIF · TIFF · TGA · ICO · <strong className="text-slate-400">PDF</strong></p>
            <p>SVG → PNG · JPG · WebP · BMP · <strong className="text-slate-400">PDF</strong></p>
            <p>PDF → <strong className="text-slate-400">TXT · HTML</strong> · Division par pages</p>
            <p>TXT / MD / HTML → <strong className="text-slate-400">PDF · HTML · TXT</strong></p>
            <p>DOCX · DOC → <strong className="text-slate-400">TXT · HTML · PDF</strong></p>
            <p>PPTX · PPT → <strong className="text-slate-400">TXT · PDF</strong></p>
            <p>XLSX · XLS · ODS → <strong className="text-slate-400">CSV · JSON · TXT · PDF</strong></p>
            <p>CSV → <strong className="text-slate-400">JSON · XLSX · TXT · PDF</strong></p>
            <p>JSON → <strong className="text-slate-400">CSV · TXT</strong></p>
          </div>
        )}
      </main>

      {/* Panels */}
      {showHistory && (
        <History
          items={history}
          onClear={clearHistory}
          onClose={() => setShowHistory(false)}
        />
      )}
      {showMergePDF && (
        <MergePDF onClose={() => setShowMergePDF(false)} />
      )}
      {mergePromptIds && (
        <MergePromptModal
          fileIds={mergePromptIds}
          allFiles={files}
          onClose={() => setMergePromptIds(null)}
        />
      )}

      {/* Toast notification */}
      {toast && (
        <div className={`fixed bottom-5 left-1/2 -translate-x-1/2 px-4 py-2.5 rounded-xl text-sm shadow-xl z-50 border transition-all
          ${toast.ok
            ? "bg-green-950/95 border-green-800 text-green-300"
            : "bg-red-950/95 border-red-800 text-red-300"}`}>
          {toast.msg}
        </div>
      )}
    </div>
  );
}
