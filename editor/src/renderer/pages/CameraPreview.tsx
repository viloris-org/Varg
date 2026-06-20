import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from '../i18n';
import { viewportReadback } from '../api';

// ─── Types ──────────────────────────────────────────────────────────────────

interface CameraPreviewProps {
  entityId: string;
  /** Width of the preview canvas in pixels */
  width?: number;
  /** Height of the preview canvas in pixels */
  height?: number;
}

// ─── Component ──────────────────────────────────────────────────────────────

/**
 * Renders a small live preview of what the selected camera entity sees.
 * Polls the backend for rendered frames at a low frequency (500ms)
 * to avoid excessive GPU load.
 */
export default function CameraPreview({
  entityId,
  width = 160,
  height = 120,
}: CameraPreviewProps) {
  const { t } = useTranslation();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const contextRef = useRef<CanvasRenderingContext2D | null>(null);
  const activeRef = useRef(true);
  const lastVersionRef = useRef<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    activeRef.current = true;

    const poll = async () => {
      if (!activeRef.current || !canvasRef.current) return;

      try {
        const buffer = await viewportReadback({
          width,
          height,
          lastVersion: lastVersionRef.current ?? undefined,
          entityId,
        });

        if (!activeRef.current || !canvasRef.current) return;

        const uint8 = new Uint8Array(buffer);
        const header = new Uint32Array(uint8.buffer, uint8.byteOffset, 2);
        const w = header[0];
        const h = header[1];

        if (w > 0 && h > 0) {
          const canvas = canvasRef.current;
          if (canvas.width !== w || canvas.height !== h) {
            canvas.width = w;
            canvas.height = h;
            contextRef.current = null;
          }
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
        setError(e instanceof Error ? e.message : String(e));
      }

      // Low-frequency poll
      if (activeRef.current) {
        setTimeout(poll, 500);
      }
    };

    poll();
    return () => { activeRef.current = false; };
  }, [entityId, width, height]);

  return (
    <div className="camera-preview-container">
      <div className="camera-preview-label">{t('camera_preview')}</div>
      {error && <div className="camera-preview-error">{error}</div>}
      <canvas
        ref={canvasRef}
        className="camera-preview-canvas"
        width={width}
        height={height}
      />
    </div>
  );
}
