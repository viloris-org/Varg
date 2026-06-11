// ─── Centralized SVG Icon Components ─────────────────────────────────────────
// All editor icons in one place. Import from here, not duplicate in JSX.

import React from 'react';

interface IconProps {
  size?: number;
  className?: string;
}

type IconFn = (props?: IconProps) => React.ReactNode;

// ─── Utility wrapper ───────────────────────────────────────────────────────

function icon(
  viewBox: string,
  children: React.ReactNode,
): IconFn {
  const Cmp = ({ size = 14, className }: IconProps = {}) => (
    <svg viewBox={viewBox} fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" width={size} height={size} className={className}>
      {children}
    </svg>
  );
  return Cmp;
}

function iconFill(
  viewBox: string,
  children: React.ReactNode,
): IconFn {
  const Cmp = ({ size = 14, className }: IconProps = {}) => (
    <svg viewBox={viewBox} width={size} height={size} className={className}>
      {children}
    </svg>
  );
  return Cmp;
}

// ─── Action Icons ──────────────────────────────────────────────────────────

export const IconX = icon('0 0 24 24', <>
  <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
</>);

export const IconCheck = icon('0 0 24 24', <>
  <polyline points="20 6 9 17 4 12" />
</>);

export const IconPlus = icon('0 0 24 24', <>
  <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
</>);

export const IconMinus = icon('0 0 24 24', <>
  <line x1="5" y1="12" x2="19" y2="12" />
</>);

export const IconTrash = icon('0 0 24 24', <>
  <polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
</>);

export const IconRefresh = icon('0 0 24 24', <>
  <polyline points="23 4 23 10 17 10" /><polyline points="1 20 1 14 7 14" />
  <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
</>);

export const IconSend = icon('0 0 24 24', <>
  <line x1="22" y1="2" x2="11" y2="13" /><polygon points="22 2 15 22 11 13 2 9 22 2" />
</>);

export const IconEdit = icon('0 0 24 24', <>
  <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
  <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
</>);

export const IconCopy = icon('0 0 24 24', <>
  <rect x="9" y="9" width="13" height="13" rx="2" ry="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
</>);

// ─── Navigation Icons ──────────────────────────────────────────────────────

export const IconFolder = icon('0 0 24 24', <>
  <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
</>);

export const IconFile = icon('0 0 24 24', <>
  <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
  <polyline points="14 2 14 8 20 8" />
</>);

export const IconImage = icon('0 0 24 24', <>
  <rect x="3" y="3" width="18" height="18" rx="2" ry="2" /><circle cx="8.5" cy="8.5" r="1.5" /><polyline points="21 15 16 10 5 21" />
</>);

export const IconCode = icon('0 0 24 24', <>
  <polyline points="16 18 22 12 16 6" /><polyline points="8 6 2 12 8 18" />
</>);

// ─── File Type Icons ───────────────────────────────────────────────────────

export const IconAudio = icon('0 0 24 24', <>
  <path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" />
</>);

export const IconShader = icon('0 0 24 24', <>
  <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" />
</>);

export const IconMaterial = icon('0 0 24 24', <>
  <circle cx="12" cy="12" r="10" /><circle cx="12" cy="12" r="6" /><circle cx="12" cy="12" r="2" />
</>);

export const IconModel = icon('0 0 24 24', <>
  <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z" />
</>);

export const IconScript = icon('0 0 24 24', <>
  <polyline points="16 18 22 12 16 6" /><polyline points="8 6 2 12 8 18" />
</>);

// ─── Status Icons ──────────────────────────────────────────────────────────

export const IconAlertCircle = icon('0 0 24 24', <>
  <circle cx="12" cy="12" r="10" /><line x1="12" y1="8" x2="12" y2="12" /><line x1="12" y1="16" x2="12.01" y2="16" />
</>);

export const IconAlertTriangle = icon('0 0 24 24', <>
  <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
  <line x1="12" y1="9" x2="12" y2="13" /><line x1="12" y1="17" x2="12.01" y2="17" />
</>);

export const IconInfo = icon('0 0 24 24', <>
  <circle cx="12" cy="12" r="10" /><line x1="12" y1="16" x2="12" y2="12" /><line x1="12" y1="8" x2="12.01" y2="8" />
</>);

export const IconLoader = icon('0 0 24 24', <>
  <line x1="12" y1="2" x2="12" y2="6" /><line x1="12" y1="18" x2="12" y2="22" />
  <line x1="4.93" y1="4.93" x2="7.76" y2="7.76" /><line x1="16.24" y1="16.24" x2="19.07" y2="19.07" />
  <line x1="2" y1="12" x2="6" y2="12" /><line x1="18" y1="12" x2="22" y2="12" />
  <line x1="4.93" y1="19.07" x2="7.76" y2="16.24" /><line x1="16.24" y1="7.76" x2="19.07" y2="4.93" />
</>);

export const IconBrain = icon('0 0 24 24', <>
  <path d="M9.5 2A5.5 5.5 0 0 0 4 7.5c0 1.33.47 2.55 1.26 3.5H4a3 3 0 0 0-3 3v.5c0 1.1.9 2 2 2h1.27A5.48 5.48 0 0 0 9.5 22a5.48 5.48 0 0 0 5.23-6H16a3 3 0 0 0 3-3v-.5c0-1.1.9-2 2-2h-1.26A5.49 5.49 0 0 0 20 7.5 5.5 5.5 0 0 0 14.5 2h-5z" />
  <path d="M12 2v20" /><path d="M9 7h6" /><path d="M9 12h6" /><path d="M9 17h6" />
</>);

// ─── UI / Tool Icons ──────────────────────────────────────────────────────

export const IconChevronDown = icon('0 0 24 24', <>
  <polyline points="6 9 12 15 18 9" />
</>);

export const IconChevronRight = icon('0 0 24 24', <>
  <polyline points="9 18 15 12 9 6" />
</>);

export const IconChevronLeft = icon('0 0 24 24', <>
  <polyline points="15 18 9 12 15 6" />
</>);

export const IconChevronUp = icon('0 0 24 24', <>
  <polyline points="18 15 12 9 6 15" />
</>);

export const IconMenu = icon('0 0 24 24', <>
  <line x1="3" y1="12" x2="21" y2="12" /><line x1="3" y1="6" x2="21" y2="6" /><line x1="3" y1="18" x2="21" y2="18" />
</>);

export const IconSearch = icon('0 0 24 24', <>
  <circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" />
</>);

export const IconSettings = icon('0 0 24 24', <>
  <circle cx="12" cy="12" r="3" />
  <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
</>);

// ─── Bot / AI Icons ────────────────────────────────────────────────────────

export const IconBot = icon('0 0 24 24', <>
  <rect x="3" y="8" width="18" height="12" rx="2" ry="2" /><circle cx="9" cy="14" r="1.5" fill="currentColor" stroke="none" /><circle cx="15" cy="14" r="1.5" fill="currentColor" stroke="none" />
  <line x1="12" y1="2" x2="12" y2="8" /><circle cx="12" cy="2" r="1.5" /><line x1="1" y1="14" x2="3" y2="14" /><line x1="21" y1="14" x2="23" y2="14" />
</>);

export const IconSparkles = icon('0 0 24 24', <>
  <path d="M12 3l1.3 4.3L17 6l-2.7 3.7L17 13l-3.7-1.3L12 16l-1.3-4.3L7 13l2.7-3.7L7 6l3.7 1.3z" />
  <path d="M8 3l.5 1.5L10 5l-1.5.5L8 7l-.5-1.5L6 5l1.5-.5z" />
</>);

// ─── Sidebar Icons ─────────────────────────────────────────────────────────

export const IconProjects = icon('0 0 24 24', <>
  <rect x="3" y="3" width="7" height="7" /><rect x="14" y="3" width="7" height="7" />
  <rect x="3" y="14" width="7" height="7" /><rect x="14" y="14" width="7" height="7" />
</>);

export const IconInstalls = icon('0 0 24 24', <>
  <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z" />
  <polyline points="3.27 6.96 12 12.01 20.73 6.96" /><line x1="12" y1="22.08" x2="12" y2="12" />
</>);

// ─── Theme Icons ───────────────────────────────────────────────────────────

export const IconSun = icon('0 0 24 24', <>
  <circle cx="12" cy="12" r="5" /><line x1="12" y1="1" x2="12" y2="3" /><line x1="12" y1="21" x2="12" y2="23" />
  <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" /><line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
  <line x1="1" y1="12" x2="3" y2="12" /><line x1="21" y1="12" x2="23" y2="12" />
  <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" /><line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
</>);

export const IconMoon = icon('0 0 24 24', <>
  <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
</>);

export const IconMonitor = icon('0 0 24 24', <>
  <rect x="2" y="3" width="20" height="14" rx="2" ry="2" /><line x1="8" y1="21" x2="16" y2="21" /><line x1="12" y1="17" x2="12" y2="21" />
</>);

// ─── Editor Tool Icons (for Menu icons) ────────────────────────────────────

export const IconSave = icon('0 0 24 24', <>
  <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" />
  <polyline points="17 21 17 13 7 13 7 21" /><polyline points="7 3 7 8 15 8" />
</>);

export const IconUndo = icon('0 0 24 24', <>
  <polyline points="1 4 1 10 7 10" /><path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10" />
</>);

export const IconRedo = icon('0 0 24 24', <>
  <polyline points="23 4 23 10 17 10" /><path d="M20.49 15a9 9 0 1 1-2.13-9.36L23 10" />
</>);

export const IconPlay = icon('0 0 24 24', <>
  <polygon points="5 3 19 12 5 21 5 3" />
</>);

export const IconStop = icon('0 0 24 24', <>
  <rect x="4" y="4" width="16" height="16" rx="2" />
</>);

export const IconPause = icon('0 0 24 24', <>
  <rect x="6" y="4" width="4" height="16" /><rect x="14" y="4" width="4" height="16" />
</>);

// ─── Transform Tool Icons ──────────────────────────────────────────────────

export const IconMove = icon('0 0 24 24', <>
  <polyline points="5 9 2 12 5 15" /><polyline points="9 5 12 2 15 5" />
  <polyline points="15 19 12 22 9 19" /><polyline points="19 9 22 12 19 15" />
  <line x1="2" y1="12" x2="22" y2="12" /><line x1="12" y1="2" x2="12" y2="22" />
</>);

export const IconRotate = icon('0 0 24 24', <>
  <polyline points="23 4 23 10 17 10" />
  <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
</>);

export const IconScale = icon('0 0 24 24', <>
  <polyline points="15 3 21 3 21 9" />
  <polyline points="9 21 3 21 3 15" />
  <line x1="21" y1="3" x2="14" y2="10" />
  <line x1="3" y1="21" x2="10" y2="14" />
</>);

export const IconView = icon('0 0 24 24', <>
  <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
  <circle cx="12" cy="12" r="3" />
</>);

// ─── Package / Empty State ─────────────────────────────────────────────────

export const IconPackage = icon('0 0 24 24', <>
  <line x1="16.5" y1="9.4" x2="7.5" y2="4.21" /><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z" />
  <polyline points="3.27 6.96 12 12.01 20.73 6.96" /><line x1="12" y1="22.08" x2="12" y2="12" />
</>);

export const IconEmpty = ({ size = 48, className }: IconProps) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" width={size} height={size} className={className}>
    <rect x="2" y="3" width="20" height="14" rx="2" ry="2" /><line x1="8" y1="21" x2="16" y2="21" /><line x1="12" y1="17" x2="12" y2="21" />
  </svg>
);

// ─── Aster Logo ────────────────────────────────────────────────────────────

export const AsterLogo = ({ size = 24, className }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 16 16" className={className}>
    <polygon points="8,1 15,5 15,11 8,15 1,11 1,5" fill="#22C55E" opacity="0.9" />
  </svg>
);

// ─── Helper: map asset kind to icon ───────────────────────────────────────

export function assetIcon(kind: string): React.ReactNode {
  const k = kind.toLowerCase();
  if (k.includes('texture') || k.includes('image') || k.includes('sprite')) return <IconImage size={14} />;
  if (k.includes('script') || k.includes('shader')) return <IconCode size={14} />;
  if (k.includes('audio') || k.includes('sound') || k.includes('music')) return <IconAudio size={14} />;
  if (k.includes('material')) return <IconMaterial size={14} />;
  if (k.includes('mesh') || k.includes('model')) return <IconModel size={14} />;
  return <IconFile size={14} />;
}
