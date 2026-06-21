type ButtonVariant = 'primary' | 'secondary' | 'danger' | 'ghost';
type ButtonSize = 'md' | 'sm';
type ToolButtonVariant = 'default' | 'play';
type ToolButtonSize = 'default' | 'toolbar' | 'icon' | 'sm';
type BadgeVariant = 'green' | 'gray';
type AiPlanBadgeVariant = 'write' | 'read';
type AiPlanItemButtonVariant = 'allow' | 'deny';
type CopilotStatusBadgeVariant = 'planning' | 'ready' | 'executing' | 'complete' | 'error';
type TaskOperationPermissionKind = 'write' | 'read' | 'command';

const BUTTON_BASE = 'inline-flex items-center gap-1.5 rounded-[var(--radius-md)] border border-transparent px-3.5 py-[7px] font-[var(--font-sans)] text-xs font-semibold leading-none shadow-[var(--shadow-sm)] transition-all duration-[var(--transition-fast)] ease-in cursor-pointer disabled:pointer-events-none disabled:cursor-default disabled:opacity-40';

const BUTTON_VARIANTS: Record<ButtonVariant, string> = {
  primary: 'border-[var(--brand)] bg-[linear-gradient(180deg,var(--brand),var(--brand-hover))] text-white enabled:hover:border-[var(--brand-hover)] enabled:hover:brightness-110',
  secondary: 'border-[var(--border)] bg-[var(--bg-elevated)] text-[var(--text-secondary)] enabled:hover:border-[var(--border-light)] enabled:hover:bg-[var(--bg-hover)] enabled:hover:text-[var(--text-primary)]',
  danger: 'border-[var(--border)] bg-transparent text-[var(--danger)] enabled:hover:border-[var(--danger)] enabled:hover:bg-[var(--danger-dim)]',
  ghost: 'border-transparent bg-transparent text-[var(--text-secondary)] enabled:hover:bg-[var(--bg-hover)] enabled:hover:text-[var(--text-primary)]',
};

const BUTTON_SIZES: Record<ButtonSize, string> = {
  md: '',
  sm: 'px-2.5 py-1 text-[11px]',
};

export function buttonClass(
  variant: ButtonVariant = 'secondary',
  size: ButtonSize = 'md',
  extra = '',
): string {
  return [BUTTON_BASE, BUTTON_VARIANTS[variant], BUTTON_SIZES[size], extra].filter(Boolean).join(' ');
}

const TOOL_BUTTON_BASE = 'inline-flex cursor-pointer items-center justify-center rounded-[var(--radius-sm)] border border-transparent bg-transparent px-2 py-[2px] font-[var(--font-sans)] text-xs text-[var(--text-secondary)] transition-all duration-150 enabled:hover:border-[var(--border-light)] enabled:hover:bg-[var(--bg-hover)] enabled:hover:text-[var(--text-primary)] disabled:cursor-default disabled:opacity-35';
const TOOL_BUTTON_VARIANTS: Record<ToolButtonVariant, string> = {
  default: '',
  play: 'font-semibold text-[var(--accent)] enabled:hover:border-[var(--accent)] enabled:hover:bg-[var(--accent-dim)] enabled:hover:text-[var(--accent)]',
};
const TOOL_BUTTON_SIZES: Record<ToolButtonSize, string> = {
  default: '',
  toolbar: 'h-6 min-w-7 px-[7px]',
  icon: 'h-[22px] min-w-[26px] px-1 py-0',
  sm: 'h-5 px-1.5 py-px text-[10px] font-semibold uppercase tracking-[0.3px]',
};

export function toolButtonClass({
  variant = 'default',
  size = 'default',
  active = false,
  extra = '',
}: {
  variant?: ToolButtonVariant;
  size?: ToolButtonSize;
  active?: boolean;
  extra?: string;
} = {}): string {
  return [
    TOOL_BUTTON_BASE,
    TOOL_BUTTON_VARIANTS[variant],
    TOOL_BUTTON_SIZES[size],
    active ? 'border-[var(--accent)] bg-[var(--accent-dim)] text-[var(--accent)]' : '',
    extra,
  ].filter(Boolean).join(' ');
}

export const modalOverlayClass = 'fixed inset-0 z-[100] flex items-center justify-center bg-black/70 backdrop-blur-[8px] animate-[fadeIn_150ms_ease]';
export function modalClass(width = 'w-[540px]'): string {
  return `${width} max-h-[85vh] max-w-[90vw] overflow-y-auto rounded-[var(--radius-xl)] border border-[var(--border-light)] bg-[var(--bg-elevated)] shadow-[var(--shadow-lg)] animate-[slideUp_200ms_ease]`;
}
export const modalHeaderClass = 'flex items-center justify-between px-6 pt-5';
export const modalTitleClass = 'text-[17px] font-semibold text-[var(--text-primary)]';
export const modalCloseButtonClass = 'flex h-7 w-7 cursor-pointer items-center justify-center rounded-[var(--radius-sm)] border-0 bg-transparent text-[var(--text-muted)] transition-all duration-[120ms] ease-in hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]';
export const modalBodyClass = 'px-6 py-5';
export const modalFooterClass = 'flex items-center justify-end gap-2 px-6 pb-5';

export const formGroupClass = 'mb-4';
export const formLabelClass = 'mb-1.5 block text-[11px] font-semibold uppercase tracking-[0.4px] text-[var(--text-muted)]';
export const formInputClass = 'w-full rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] px-3 py-2 font-[var(--font-sans)] text-[13px] text-[var(--text-primary)] outline-none shadow-[inset_0_1px_0_rgba(255,255,255,0.025)] transition-[border-color,background-color,box-shadow] duration-[120ms] ease-in placeholder:text-[var(--text-muted)] focus:border-[var(--accent)] focus:shadow-[0_0_0_3px_var(--accent-dim)]';
export const formSelectClass = 'w-full cursor-pointer rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] px-3 py-2 font-[var(--font-sans)] text-[13px] text-[var(--text-primary)] outline-none transition-[border-color,box-shadow] duration-[120ms] ease-in focus:border-[var(--accent)] focus:shadow-[0_0_0_3px_var(--accent-dim)]';
export const formErrorClass = 'mt-1 text-xs text-[var(--danger)]';

export const templateGridClass = 'mb-4 grid grid-cols-2 gap-2.5';
export function templateCardClass(selected: boolean): string {
  return selected
    ? 'cursor-pointer rounded-[var(--radius-lg)] border border-[var(--accent)] bg-[var(--accent-dim)] p-4 shadow-[var(--shadow-sm)] transition-all duration-[120ms] ease-in'
    : 'cursor-pointer rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--bg-elevated)] p-4 shadow-[var(--shadow-sm)] transition-all duration-[120ms] ease-in hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)]';
}
export const templateCardIconClass = 'mb-2 block h-6 w-6 text-[var(--accent)]';
export const templateCardTitleClass = 'mb-1 text-sm font-semibold text-[var(--text-primary)]';
export const templateCardDescClass = 'text-[11px] leading-[1.4] text-[var(--text-muted)]';

export const locationInputRowClass = 'flex items-center gap-2';
export const warningPanelClass = 'mb-4 flex items-start gap-3 rounded-[var(--radius-md)] border border-[rgba(239,68,68,0.2)] bg-[var(--danger-dim)] px-3.5 py-3';
export const warningPanelIconClass = 'mt-px flex-shrink-0 text-[var(--danger)]';
export const warningPanelTextClass = 'text-[13px] leading-[1.5] text-[var(--text-primary)] [&_strong]:text-[var(--danger)]';

export const installCardClass = 'flex items-center gap-3.5 rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--bg-elevated)] px-4 py-3.5 shadow-[var(--shadow-sm)] transition-[border-color,background-color,transform] duration-[120ms] ease-in hover:-translate-y-px hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)]';
export const installIconClass = 'flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] bg-[var(--accent-dim)] text-[var(--accent)]';
export const installInfoClass = 'min-w-0 flex-1';
export const installVersionClass = 'text-sm font-semibold text-[var(--text-primary)]';
export const installPathClass = 'mt-px overflow-hidden text-ellipsis whitespace-nowrap text-[11px] text-[var(--text-muted)]';
export const installBadgesClass = 'flex flex-shrink-0 gap-1.5';
export const badgeClass = (variant: BadgeVariant): string => {
  const base = 'rounded-[10px] px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.3px]';
  return variant === 'green'
    ? `${base} bg-[var(--accent-dim)] text-[var(--accent)]`
    : `${base} bg-[var(--bg-hover)] text-[var(--text-muted)]`;
};

export const settingsSectionTitleClass = 'mb-3 text-xs font-semibold uppercase tracking-[0.5px] text-[var(--text-muted)]';
export const settingsLabelClass = 'text-[13px] font-medium text-[var(--text-primary)]';
export const settingsDescClass = 'mt-0.5 text-[11px] text-[var(--text-muted)]';
export const settingsInputClass = 'h-8 w-full rounded-[var(--radius-sm)] border border-[var(--border-light)] bg-[var(--bg-surface)] px-2.5 font-[var(--font-sans)] text-xs text-[var(--text-primary)] outline-none transition-[border-color,background-color] duration-[120ms] ease-in hover:border-[var(--text-muted)] focus:border-[var(--accent)]';
export const settingsSelectClass = 'h-8 w-full appearance-none rounded-[var(--radius-sm)] border border-[var(--border-light)] bg-[var(--bg-surface)] bg-[url(data:image/svg+xml,%3Csvg%20xmlns=%27http://www.w3.org/2000/svg%27%20width=%2712%27%20height=%2712%27%20viewBox=%270%200%2024%2024%27%20fill=%27none%27%20stroke=%27%23A1A1AA%27%20stroke-width=%272%27%20stroke-linecap=%27round%27%20stroke-linejoin=%27round%27%3E%3Cpolyline%20points=%276%209%2012%2015%2018%209%27/%3E%3C/svg%3E)] bg-[position:right_10px_center] bg-no-repeat py-0 pr-8 pl-2.5 font-[var(--font-sans)] text-xs text-[var(--text-primary)] outline-none transition-[border-color,background-color] duration-[120ms] ease-in hover:border-[var(--text-muted)] focus:border-[var(--accent)]';
export const settingsSelectOptionClass = 'bg-[var(--bg-surface)] text-[var(--text-primary)]';

export function themeOptionClass(active: boolean): string {
  return active
    ? 'flex h-[26px] min-w-0 cursor-pointer items-center justify-center whitespace-nowrap rounded-[3px] border-0 bg-[var(--brand)] px-2 font-[var(--font-sans)] text-xs leading-none text-[var(--bg-base)] shadow-[var(--shadow-sm)] transition-colors duration-[120ms] ease-in hover:bg-[var(--brand-hover)]'
    : 'flex h-[26px] min-w-0 cursor-pointer items-center justify-center whitespace-nowrap rounded-[3px] border-0 bg-transparent px-2 font-[var(--font-sans)] text-xs leading-none text-[var(--text-muted)] transition-colors duration-[120ms] ease-in hover:text-[var(--text-primary)]';
}

export function projectCardClass(selected: boolean): string {
  return selected
    ? 'relative flex cursor-pointer items-center gap-3.5 rounded-[var(--radius-lg)] border border-[var(--brand)] bg-[var(--brand-dim)] px-4 py-3.5 shadow-[var(--shadow-md)] transition-all duration-200 ease-in hover:brightness-110'
    : 'relative flex cursor-pointer items-center gap-3.5 rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--bg-elevated)] px-4 py-3.5 shadow-[var(--shadow-sm)] transition-all duration-200 ease-in hover:-translate-y-px hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)]';
}
export const projectAvatarClass = 'flex h-[42px] w-[42px] flex-shrink-0 items-center justify-center rounded-[var(--radius-md)] font-[var(--font-sans)] text-[15px] font-bold text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.24),0_8px_18px_rgba(0,0,0,0.22)]';
export const projectInfoClass = 'min-w-0 flex-1';
export const projectNameClass = 'overflow-hidden text-ellipsis whitespace-nowrap text-sm font-semibold text-[var(--text-primary)]';
export const projectPathClass = 'mt-px overflow-hidden text-ellipsis whitespace-nowrap text-[11px] text-[var(--text-muted)]';
export const projectMetaClass = 'mt-[3px] flex items-center gap-2 text-[11px] text-[var(--text-muted)]';
export const projectMetaDotClass = 'h-[3px] w-[3px] rounded-full bg-[var(--text-muted)]';
export const projectFolderButtonClass = 'flex h-[30px] w-[30px] flex-shrink-0 cursor-pointer items-center justify-center rounded-[var(--radius-md)] border border-transparent bg-transparent text-[var(--text-muted)] transition-all duration-[120ms] ease-in hover:border-[var(--border-light)] hover:bg-[var(--bg-base)] hover:text-[var(--text-primary)]';
export const projectPanelIconButtonClass = 'flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-[3px] border-0 bg-transparent text-[var(--text-secondary)] transition-all duration-[var(--transition-fast)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]';
export const projectPanelSearchInputClass = 'w-full rounded-[3px] border border-[var(--border)] bg-[var(--bg-base)] bg-[url(data:image/svg+xml,%3Csvg%20xmlns=%27http://www.w3.org/2000/svg%27%20width=%2711%27%20height=%2711%27%20viewBox=%270%200%2024%2024%27%20fill=%27none%27%20stroke=%27%2364748B%27%20stroke-width=%272%27%20stroke-linecap=%27round%27%20stroke-linejoin=%27round%27%3E%3Ccircle%20cx=%2711%27%20cy=%2711%27%20r=%278%27/%3E%3Cline%20x1=%2721%27%20y1=%2721%27%20x2=%2716.65%27%20y2=%2716.65%27/%3E%3C/svg%3E)] bg-[position:6px_center] bg-no-repeat py-[3px] pr-2 pl-[22px] font-[var(--font-sans)] text-[11px] text-[var(--text-primary)] outline-none transition-[border-color] duration-[var(--transition-fast)] focus:border-[var(--accent)]';
export const projectTreeRenameInputClass = 'min-w-0 flex-1 rounded-[2px] border border-[var(--accent)] bg-[var(--bg-surface)] px-1 py-px font-[var(--font-sans)] text-xs text-[var(--text-primary)] outline-none';

export const cameraPreviewContainerClass = 'py-2 text-center';
export const cameraPreviewLabelClass = 'mb-1 text-[10px] uppercase tracking-[0.05em] text-[var(--text-muted)]';
export const cameraPreviewCanvasClass = 'rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)]';

export const viewportGridClass = 'pointer-events-none absolute inset-0 z-[1] opacity-60 bg-[linear-gradient(rgba(255,255,255,0.025)_1px,transparent_1px),linear-gradient(90deg,rgba(255,255,255,0.025)_1px,transparent_1px)] bg-[length:40px_40px]';
export const orientationGizmoClass = 'pointer-events-none absolute top-2 right-2 z-[2] h-20 w-20 [&_svg]:pointer-events-auto [&_svg]:cursor-pointer [&_svg]:drop-shadow-[0_1px_3px_rgba(0,0,0,0.5)] [&_svg_text]:pointer-events-none [&_svg_text]:select-none';
export const orientationGizmoAxisClass = 'cursor-pointer pointer-events-[bounding-box] hover:opacity-100';

export const toolbarExtrasClass = 'flex h-full items-center gap-1';
export const toolbarGroupClass = 'flex h-full items-center gap-0.5';
export const toolbarGroupRelativeClass = `${toolbarGroupClass} relative`;
export const toolbarSelectClass = 'cursor-pointer rounded-[2px] border border-[var(--border)] bg-[var(--bg-base)] px-1 py-0.5 font-[var(--font-sans)] text-[11px] text-[var(--text-primary)] outline-none transition-[border-color] duration-[var(--transition-fast)] focus:border-[var(--accent)]';
export const toolbarSeparatorClass = 'mx-1 h-4 w-px bg-[var(--border)]';
export const contextMenuClass = 'min-w-[120px] rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-surface)] py-1 shadow-[var(--shadow-lg)] animate-[fadeIn_100ms_ease]';
export const contextMenuItemClass = 'flex w-full cursor-pointer items-center gap-1.5 border-0 bg-transparent px-3 py-[5px] text-left font-[var(--font-sans)] text-xs text-[var(--text-primary)] transition-[background,color] duration-[var(--transition-fast)] hover:bg-[var(--bg-hover)]';
export const contextMenuDangerItemClass = `${contextMenuItemClass} text-[var(--danger)] hover:bg-[var(--danger-dim)]`;
export const contextMenuSeparatorClass = 'mx-2 my-1 h-px bg-[var(--border)]';

export const hubEmptyClass = 'col-span-full flex flex-col items-center justify-center px-8 py-16 text-center';
export const hubEmptyIconClass = 'mb-4 h-12 w-12 text-[var(--text-muted)] opacity-50';
export const hubEmptyTitleClass = 'mb-1.5 text-base font-semibold text-[var(--text-secondary)]';
export const hubEmptyTextClass = 'max-w-xs text-[13px] leading-normal text-[var(--text-muted)]';

export const aiEntityContextCompBadgeClass = 'rounded-[3px] bg-[var(--accent-dim)] px-1.5 py-px text-[10px] font-medium text-[var(--accent)]';

export function aiPlanBadgeClass(variant: AiPlanBadgeVariant): string {
  const base = 'flex h-[18px] min-w-[22px] flex-shrink-0 items-center justify-center rounded px-[3px] text-[10px] font-bold';
  return variant === 'write'
    ? `${base} bg-[#f59e0b20] text-[#f59e0b]`
    : `${base} bg-[#10b98120] text-[#10b981]`;
}

export function aiPlanItemButtonClass(variant: AiPlanItemButtonVariant): string {
  const base = 'cursor-pointer rounded border px-2 py-0.5 font-inherit text-[10px] font-semibold leading-[1.4] transition-colors duration-[120ms] ease-in';
  return variant === 'allow'
    ? `${base} border-[rgba(16,185,129,0.35)] bg-[rgba(16,185,129,0.1)] text-[#10b981] hover:border-[#10b981] hover:bg-[rgba(16,185,129,0.2)]`
    : `${base} border-[rgba(239,68,68,0.3)] bg-[rgba(239,68,68,0.08)] text-[#ef4444] hover:border-[#ef4444] hover:bg-[rgba(239,68,68,0.18)]`;
}

const COPILOT_STATUS_BADGE_BASE = 'ml-auto rounded-[3px] px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-[0.3px]';
const COPILOT_STATUS_BADGE_VARIANTS: Record<CopilotStatusBadgeVariant, string> = {
  planning: 'bg-[var(--info)] text-white',
  ready: 'bg-[var(--accent)] text-white',
  executing: 'bg-[var(--warning)] text-black',
  complete: 'bg-[var(--accent-dim)] text-[var(--accent)]',
  error: 'bg-[var(--danger-dim)] text-[var(--danger)]',
};

export function copilotStatusBadgeClass(variant: CopilotStatusBadgeVariant): string {
  return `${COPILOT_STATUS_BADGE_BASE} ${COPILOT_STATUS_BADGE_VARIANTS[variant]}`;
}

export const copilotPlanBadgeReadClass = 'flex h-3.5 w-3.5 flex-shrink-0 items-center justify-center text-[var(--accent)]';
export const copilotPlanBadgeClass = 'flex-shrink-0 rounded-[3px] bg-[var(--warning)] px-[5px] py-px text-[10px] font-semibold uppercase tracking-[0.3px] text-black';

export const productEmptyClass = 'product-empty flex min-h-[220px] flex-col items-center justify-center gap-[7px] text-[var(--text-muted)]';
export const productEmptyIconClass = 'h-5 w-5 text-[var(--text-muted)]';
export const productEmptyTitleClass = 'text-xs font-semibold text-[var(--text-secondary)]';
export const productEmptyTextClass = 'text-[10px]';

const TASK_OPERATION_PERMISSION_LABEL_COLORS: Record<TaskOperationPermissionKind, string> = {
  write: 'text-[#fbbf24]',
  read: 'text-[#22c55e]',
  command: 'text-[var(--text-secondary)]',
};

export function taskOperationPermissionLabelClass(kind: string): string {
  const color = TASK_OPERATION_PERMISSION_LABEL_COLORS[kind as TaskOperationPermissionKind] ?? 'text-[var(--text-muted)]';
  return `${color}`;
}
