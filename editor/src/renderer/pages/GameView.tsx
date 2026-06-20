import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from '../i18n';
import { startPlayMode, stopPlayMode, viewportReadback } from '../api';
import { getCurrentWindow } from '@tauri-apps/api/window';

/**
 * Game View — standalone fullscreen render target launched from the editor.
 * Renders via binary IPC (raw RGBA, no PNG/base64 overhead).
 */
export default function GameView() {
  const { t } = useTranslation();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const contextRef = useRef<CanvasRenderingContext2D | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const sizeRef = useRef({ width: 1280, height: 720 });
  const isActiveRef = useRef(true);
  const [gameError, setGameError] = useState<string | null>(null);

  // Poll for frames via binary IPC with lazy rendering at ~60fps
  useEffect(() => {
    isActiveRef.current = true;
    startPlayMode().catch((err) => {
      const msg = err?.message || String(err);
      console.error('[gameview] failed to start play mode:', msg);
      setGameError(msg);
    });

    const poll = async () => {
      if (!isActiveRef.current) return;
      const { width, height } = sizeRef.current;
      try {
        const buffer = await viewportReadback({
          width, height,
          playMode: true,
        });
        if (!isActiveRef.current || !canvasRef.current) return;

        // Parse header: [width: u32 LE][height: u32 LE][RGBA pixels...]
        const uint8 = new Uint8Array(buffer);
        const header = new Uint32Array(uint8.buffer, uint8.byteOffset, 2);
        const w = header[0];
        const h = header[1];

        // w === 0 means "no change" (GPU render skipped on backend)
        if (w > 0 && h > 0) {
          const canvas = canvasRef.current;
          const ctx = contextRef.current ?? canvas.getContext('2d');
          contextRef.current = ctx;
          if (ctx) {
            const pixelOffset = uint8.byteOffset + 8;
            const pixelBytes = w * h * 4;
            const imageData = new ImageData(
              new Uint8ClampedArray(uint8.buffer, pixelOffset, pixelBytes),
              w, h,
            );
            ctx.putImageData(imageData, 0, 0);
          }
        }
      } catch (e) {
        // Log errors so they are visible in devtools
        console.error('[gameview] readback error:', e);
      }
      setTimeout(poll, 16); // ~60fps target
    };

    poll();
    return () => {
      isActiveRef.current = false;
      stopPlayMode().catch((err) => {
        console.error('[gameview] failed to stop play mode:', err?.message || String(err));
      });
    };
  }, []);

  // Fill the window
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        if (width > 0 && height > 0) {
          const w = Math.round(width);
          const h = Math.round(height);
          sizeRef.current = { width: w, height: h };
          const canvas = canvasRef.current;
          if (canvas) { canvas.width = w; canvas.height = h; }
        }
      }
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  // Close on Escape — use Tauri native API
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        getCurrentWindow().close().catch((err) => {
          console.error('[gameview] failed to close window:', err?.message || String(err));
        });
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);

  return (
    <div ref={containerRef} className="gameview-container">
      {gameError && (
        <div className="gameview-error">
          <p>{t('game_start_failed')}</p>
          <pre>{gameError}</pre>
        </div>
      )}
      <canvas ref={canvasRef} className="gameview-canvas" />
    </div>
  );
}
