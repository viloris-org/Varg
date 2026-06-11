import React, { useCallback, useEffect, useRef, useState } from 'react';
import { detectLanguage, highlightCode } from './syntaxHighlight';

// ─── Types ──────────────────────────────────────────────────────────────────

interface ScriptEditorProps {
  filePath: string;
  initialContent: string;
  onSave: (path: string, content: string) => Promise<void>;
  onClose: () => void;
}

// ─── Component ──────────────────────────────────────────────────────────────

export default function ScriptEditor({ filePath, initialContent, onSave, onClose }: ScriptEditorProps) {
  const [content, setContent] = useState(initialContent);
  const [dirty, setDirty] = useState(false);
  const [findOpen, setFindOpen] = useState(false);
  const [findText, setFindText] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const highlightRef = useRef<HTMLPreElement>(null);
  const language = detectLanguage(filePath) ?? 'rhai';

  // Sync scroll between textarea and highlighted overlay
  const syncScroll = useCallback(() => {
    if (textareaRef.current && highlightRef.current) {
      highlightRef.current.scrollTop = textareaRef.current.scrollTop;
      highlightRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
  }, []);

  // Generate line numbers
  const lineCount = content.split('\n').length;
  const lineNumbers = Array.from({ length: Math.max(1, lineCount) }, (_, i) => i + 1);

  // Highlighted HTML
  const highlightedHtml = highlightCode(content, language);

  // Save handler
  const handleSave = useCallback(async () => {
    try {
      await onSave(filePath, content);
      setDirty(false);
    } catch (e) {
      console.error('[script-editor] save error:', e);
    }
  }, [filePath, content, onSave]);

  // Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // Ctrl+S → Save
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        handleSave();
        return;
      }
      // Ctrl+F → Find
      if ((e.ctrlKey || e.metaKey) && e.key === 'f') {
        e.preventDefault();
        setFindOpen(true);
        return;
      }
      // Escape → Close find
      if (e.key === 'Escape' && findOpen) {
        setFindOpen(false);
      }
    };

    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [handleSave, findOpen]);

  return (
    <div className="script-editor">
      {/* Header */}
      <div className="script-editor-header">
        <span className="script-editor-title">{filePath}</span>
        <span className={`script-editor-lang tag-${language}`}>{language}</span>
        {dirty && <span className="script-editor-dirty">●</span>}
        <div className="script-editor-actions">
          <button
            className="tool-btn"
            onClick={handleSave}
            disabled={!dirty}
            title="Save (Ctrl+S)"
          >
            Save
          </button>
          <button
            className="tool-btn"
            onClick={onClose}
            title="Close"
          >
            × Close
          </button>
        </div>
      </div>

      {/* Find bar */}
      {findOpen && (
        <div className="script-editor-find-bar">
          <input
            className="script-editor-find-input"
            type="text"
            placeholder="Find…"
            value={findText}
            onChange={(e) => setFindText(e.target.value)}
            autoFocus
          />
          <button className="tool-btn" onClick={() => setFindOpen(false)}>×</button>
        </div>
      )}

      {/* Editor area */}
      <div className="script-editor-body">
        {/* Line numbers gutter */}
        <div className="script-editor-gutter">
          {lineNumbers.map(n => (
            <div key={n} className="script-editor-gutter-line">{n}</div>
          ))}
        </div>

        {/* Code area with highlighting overlay */}
        <div className="script-editor-code-area">
          <pre
            ref={highlightRef}
            className="script-editor-highlight"
            aria-hidden="true"
          >
            <code dangerouslySetInnerHTML={{ __html: highlightedHtml + '\n' }} />
          </pre>
          <textarea
            ref={textareaRef}
            className="script-editor-textarea"
            value={content}
            onChange={(e) => {
              setContent(e.target.value);
              setDirty(true);
            }}
            onScroll={syncScroll}
            onKeyDown={(e) => {
              if (e.key === 'Tab') {
                e.preventDefault();
                const ta = e.target as HTMLTextAreaElement;
                const start = ta.selectionStart;
                const end = ta.selectionEnd;
                const newContent = content.slice(0, start) + '  ' + content.slice(end);
                setContent(newContent);
                setDirty(true);
                // Restore cursor position after React re-render
                requestAnimationFrame(() => {
                  ta.selectionStart = ta.selectionEnd = start + 2;
                });
              }
            }}
            spellCheck={false}
          />
        </div>
      </div>
    </div>
  );
}
