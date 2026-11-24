import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { BookOpen, Check, Copy, Languages, MessageSquare } from "lucide-react";
import { api, type SelectionPayload } from "../lib/tauri";

export function ToolbarWindow() {
  const [selectedText, setSelectedText] = useState("");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const unlistenPromise = listen<SelectionPayload>("selection:update", (event) => {
      setSelectedText(event.payload.text);
      setCopied(false);
    });
    return () => {
      void unlistenPromise.then((fn) => fn());
    };
  }, []);

  const handleAction = async (action: "translate" | "explain" | "ask") => {
    if (!selectedText) return;
    await api.triggerSelectionAction(action, selectedText);
  };

  const handleCopy = async () => {
    if (!selectedText) return;
    await api.copyText(selectedText);
    setCopied(true);
    setTimeout(() => {
      void api.hideWindow("toolbar");
    }, 240);
  };

  return (
    <div className="w-full h-full flex items-center justify-center p-1">
      <div className="glass-pill h-9 px-1.5 rounded-full flex items-center gap-0.5 text-[12.5px] font-medium">
        <button
          type="button"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => void handleAction("translate")}
          className="h-7 px-2.5 rounded-full flex items-center gap-1.5 transition-colors hover:bg-[var(--accent-soft)] hover:text-[var(--accent)] cursor-pointer"
          title="划词翻译"
        >
          <Languages className="w-3.5 h-3.5 shrink-0" />
          <span>翻译</span>
        </button>

        <div className="w-px h-3.5 bg-[var(--border-color)]" />

        <button
          type="button"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => void handleAction("explain")}
          className="h-7 px-2.5 rounded-full flex items-center gap-1.5 transition-colors hover:bg-[var(--accent-soft)] hover:text-[var(--accent)] cursor-pointer"
          title="划词解释"
        >
          <BookOpen className="w-3.5 h-3.5 shrink-0" />
          <span>解释</span>
        </button>

        <div className="w-px h-3.5 bg-[var(--border-color)]" />

        <button
          type="button"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => void handleAction("ask")}
          className="h-7 w-7 rounded-full flex items-center justify-center transition-colors hover:bg-[var(--accent-soft)] hover:text-[var(--accent)] cursor-pointer"
          title="发到弹窗助手提问"
        >
          <MessageSquare className="w-3.5 h-3.5" />
        </button>

        <button
          type="button"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => void handleCopy()}
          className="h-7 w-7 rounded-full flex items-center justify-center transition-colors hover:bg-[var(--accent-soft)] hover:text-[var(--accent)] cursor-pointer"
          title="复制选中文本"
        >
          {copied ? (
            <Check className="w-3.5 h-3.5 text-emerald-500" />
          ) : (
            <Copy className="w-3.5 h-3.5" />
          )}
        </button>
      </div>
    </div>
  );
}
