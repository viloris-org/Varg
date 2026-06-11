// ─── Regex-Based Syntax Highlighting ────────────────────────────────────────
// Simple tokenizer for Rhai and Python. Returns HTML with <span class="token-*"> wrappers.

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
  { pattern: RHAI_COMMENT_BLOCK, className: 'token-comment' },
  { pattern: RHAI_COMMENT_SINGLE, className: 'token-comment' },
  { pattern: RHAI_STRING, className: 'token-string' },
  { pattern: RHAI_KEYWORDS, className: 'token-keyword' },
  { pattern: RHAI_NUMBER, className: 'token-number' },
  { pattern: RHAI_OPERATOR, className: 'token-operator' },
];

// ─── Python ─────────────────────────────────────────────────────────────────

const PY_KEYWORDS = /\b(def|class|import|from|if|elif|else|while|for|in|return|break|continue|pass|raise|try|except|finally|with|as|yield|lambda|and|or|not|is|None|True|False|global|nonlocal|assert|del|async|await)\b/g;
const PY_STRING = /"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|"""[\s\S]*?"""|'''[\s\S]*?'''/g;
const PY_NUMBER = /\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b/g;
const PY_COMMENT = /#[^\n]*/g;
const PY_DECORATOR = /@\w+/g;
const PY_OPERATOR = /[+\-*/%=!<>&|^~@]+/g;

const PY_RULES: TokenRule[] = [
  { pattern: PY_STRING, className: 'token-string' },
  { pattern: PY_COMMENT, className: 'token-comment' },
  { pattern: PY_DECORATOR, className: 'token-keyword' },
  { pattern: PY_KEYWORDS, className: 'token-keyword' },
  { pattern: PY_NUMBER, className: 'token-number' },
  { pattern: PY_OPERATOR, className: 'token-operator' },
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

export function highlightCode(source: string, language: 'rhai' | 'python'): string {
  switch (language) {
    case 'rhai': return highlightRhai(source);
    case 'python': return highlightPython(source);
  }
}

export function detectLanguage(filePath: string): 'rhai' | 'python' | null {
  if (filePath.endsWith('.rhai')) return 'rhai';
  if (filePath.endsWith('.py')) return 'python';
  return null;
}
