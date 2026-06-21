// ─── Regex-Based Syntax Highlighting ────────────────────────────────────────
// Simple tokenizer for Rhai, Python, and Aster model declarations. Returns HTML with <span class="token-*"> wrappers.

// ─── Token Types ───────────────────────────────────────────────────────────

interface TokenRule {
  pattern: RegExp;
  className: string;
}

// ─── Rhai ───────────────────────────────────────────────────────────────────

const RHAI_KEYWORDS = /\b(let|const|fn|function|if|else|while|for|in|loop|return|break|continue|true|false|import|export|as|try|catch|throw|switch|case|default|private|public|static|new|this|global|typeof|is|is_def_var|and|or|not|do|until|module|eval|call|curry|shared|sync|sealed|abstract|unit|super|spawn|thread|go|defer)\b/g;
const RHAI_STRING = /"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'/g;
const RHAI_NUMBER = /\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b/g;
const RHAI_COMMENT_SINGLE = /\/\/[^\n]*/g;
const RHAI_COMMENT_BLOCK = /\/\*[\s\S]*?\*\//g;
const RHAI_OPERATOR = /[+\-*/%=!<>&|^~]+/g;

const RHAI_RULES: TokenRule[] = [
  { pattern: RHAI_COMMENT_BLOCK, className: 'text-[#546E7A] italic' },
  { pattern: RHAI_COMMENT_SINGLE, className: 'text-[#546E7A] italic' },
  { pattern: RHAI_STRING, className: 'text-[#C3E88D]' },
  { pattern: RHAI_KEYWORDS, className: 'text-[#D4D4D8] font-medium' },
  { pattern: RHAI_NUMBER, className: 'text-[#F78C6C]' },
  { pattern: RHAI_OPERATOR, className: 'text-[#A1A1AA]' },
];

// ─── Python ─────────────────────────────────────────────────────────────────

const PY_KEYWORDS = /\b(def|class|import|from|if|elif|else|while|for|in|return|break|continue|pass|raise|try|except|finally|with|as|yield|lambda|and|or|not|is|None|True|False|global|nonlocal|assert|del|async|await)\b/g;
const PY_STRING = /"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|"""[\s\S]*?"""|'''[\s\S]*?'''/g;
const PY_NUMBER = /\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b/g;
const PY_COMMENT = /#[^\n]*/g;
const PY_DECORATOR = /@\w+/g;
const PY_OPERATOR = /[+\-*/%=!<>&|^~@]+/g;

const PY_RULES: TokenRule[] = [
  { pattern: PY_STRING, className: 'text-[#C3E88D]' },
  { pattern: PY_COMMENT, className: 'text-[#546E7A] italic' },
  { pattern: PY_DECORATOR, className: 'text-[#D4D4D8] font-medium' },
  { pattern: PY_KEYWORDS, className: 'text-[#D4D4D8] font-medium' },
  { pattern: PY_NUMBER, className: 'text-[#F78C6C]' },
  { pattern: PY_OPERATOR, className: 'text-[#A1A1AA]' },
];

// ─── Aster Model Language ──────────────────────────────────────────────────

const AMDL_KEYWORDS = /\b(model|mesh|material|collider|rigidbody|socket|lod|metadata|asset|ref|primitive|static|dynamic|kinematic|true|false)\b/g;
const AMDL_CONSTRUCTORS = /\b(?:primitive|collider)\.[A-Za-z_][A-Za-z0-9_-]*\b/g;
const AMDL_STRING = /"(?:[^"\\]|\\.)*"/g;
const AMDL_NUMBER = /-?\b\d+(?:\.\d+)?(?:[a-zA-Z_%]+)?\b/g;
const AMDL_COMMENT_SINGLE = /\/\/[^\n]*|#[^\n]*/g;
const AMDL_OPERATOR = /[=:\[\]{},().]/g;

const AMDL_RULES: TokenRule[] = [
  { pattern: AMDL_COMMENT_SINGLE, className: 'text-[#546E7A] italic' },
  { pattern: AMDL_STRING, className: 'text-[#C3E88D]' },
  { pattern: AMDL_CONSTRUCTORS, className: 'text-[#82AAFF]' },
  { pattern: AMDL_KEYWORDS, className: 'text-[#D4D4D8] font-medium' },
  { pattern: AMDL_NUMBER, className: 'text-[#F78C6C]' },
  { pattern: AMDL_OPERATOR, className: 'text-[#A1A1AA]' },
];

// ─── Common HTML escaping ───────────────────────────────────────────────────

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

// ─── Tokenizer ──────────────────────────────────────────────────────────────

function tokenizeLine(line: string, rules: TokenRule[]): string {
  // Find all token matches with their positions
  interface Match {
    start: number;
    end: number;
    className: string;
    text: string;
  }

  const matches: Match[] = [];

  for (const rule of rules) {
    // Reset lastIndex for global regex
    rule.pattern.lastIndex = 0;
    let match: RegExpExecArray | null;
    while ((match = rule.pattern.exec(line)) !== null) {
      // Avoid overlapping matches — skip if this region is already covered
      const isOverlapping = matches.some(
        m => match!.index < m.end && match!.index + match![0].length > m.start
      );
      if (!isOverlapping) {
        matches.push({
          start: match.index,
          end: match.index + match[0].length,
          className: rule.className,
          text: match[0],
        });
      }
    }
  }

  // Sort by start position
  matches.sort((a, b) => a.start - b.start);

  // Build highlighted HTML
  let result = '';
  let pos = 0;
  for (const m of matches) {
    if (m.start > pos) {
      result += escapeHtml(line.slice(pos, m.start));
    }
    result += `<span class="${m.className}">${escapeHtml(m.text)}</span>`;
    pos = m.end;
  }
  result += escapeHtml(line.slice(pos));

  return result;
}

// ─── Public API ─────────────────────────────────────────────────────────────

export function highlightRhai(source: string): string {
  return source.split('\n').map(line => tokenizeLine(line, RHAI_RULES)).join('\n');
}

export function highlightPython(source: string): string {
  return source.split('\n').map(line => tokenizeLine(line, PY_RULES)).join('\n');
}

export function highlightAmdl(source: string): string {
  return source.split('\n').map(line => tokenizeLine(line, AMDL_RULES)).join('\n');
}

export type EditorLanguage = 'rhai' | 'python' | 'amdl';

export function highlightCode(source: string, language: EditorLanguage): string {
  switch (language) {
    case 'rhai': return highlightRhai(source);
    case 'python': return highlightPython(source);
    case 'amdl': return highlightAmdl(source);
  }
}

export function detectLanguage(filePath: string): EditorLanguage | null {
  if (filePath.endsWith('.aster')) return 'rhai';
  // Keep legacy .rhai projects editable during migration.
  if (filePath.endsWith('.rhai')) return 'rhai';
  if (filePath.endsWith('.py')) return 'python';
  if (filePath.endsWith('.amdl')) return 'amdl';
  return null;
}
