// ─── Regex-Based Syntax Highlighting ────────────────────────────────────────
// Simple tokenizer for Varg and Python. Returns HTML with <span class="token-*"> wrappers.

// ─── Token Types ───────────────────────────────────────────────────────────

interface TokenRule {
  pattern: RegExp;
  className: string;
}

// ─── Varg ───────────────────────────────────────────────────────────────────

const VARG_KEYWORDS = /\b(import|script|module|behavior|scene|prefab|network|model|material|audio|entity|camera|light|spawn|scatter|place|intent|layout|selector|sequence|when|action|let|var|func|if|else|for|in|while|return|break|continue|guard|true|false|nil)\b/g;
const VARG_TYPES = /\b(Float|Int|String|Bool|Vec2|Vec3|Euler|Color|Entity|EventData|Asset|Scene|Prefab|Material|AudioEvent)\??\b/g;
const VARG_ANNOTATION = /@[A-Za-z_][A-Za-z0-9_]*/g;
const VARG_STRING = /"(?:[^"\\]|\\.)*"/g;
const VARG_NUMBER = /-?\b\d+(?:\.\d+)?(?:[a-zA-Z_%]+)?\b/g;
const VARG_COMMENT_SINGLE = /\/\/[^\n]*/g;
const VARG_OPERATOR = /[+\-*/%=!<>&|^~.:,{}()[\]]+/g;

const VARG_RULES: TokenRule[] = [
  { pattern: VARG_COMMENT_SINGLE, className: 'text-[#546E7A] italic' },
  { pattern: VARG_STRING, className: 'text-[#C3E88D]' },
  { pattern: VARG_ANNOTATION, className: 'text-[#FFCB6B]' },
  { pattern: VARG_TYPES, className: 'text-[#82AAFF]' },
  { pattern: VARG_KEYWORDS, className: 'text-[#D4D4D8] font-medium' },
  { pattern: VARG_NUMBER, className: 'text-[#F78C6C]' },
  { pattern: VARG_OPERATOR, className: 'text-[#A1A1AA]' },
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

export function highlightVarg(source: string): string {
  return source.split('\n').map(line => tokenizeLine(line, VARG_RULES)).join('\n');
}

export function highlightPython(source: string): string {
  return source.split('\n').map(line => tokenizeLine(line, PY_RULES)).join('\n');
}

export type EditorLanguage = 'varg' | 'python';

export function highlightCode(source: string, language: EditorLanguage): string {
  switch (language) {
    case 'varg': return highlightVarg(source);
    case 'python': return highlightPython(source);
  }
}

export function detectLanguage(filePath: string): EditorLanguage | null {
  if (filePath.endsWith('.varg') || filePath.endsWith('.vscene') || filePath.endsWith('.vasset')) return 'varg';
  if (filePath.endsWith('.py')) return 'python';
  return null;
}
