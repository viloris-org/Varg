import React, { createContext, useContext, useState, useEffect, useCallback, useRef } from 'react';
import { rpc } from './api';

// ─── Types ──────────────────────────────────────────────────────────────────

interface TranslationEntry {
  key: string;
  value: string;
}

interface TranslationsPayload {
  locale: string;
  entries: TranslationEntry[];
}

interface I18nContextValue {
  locale: string;
  t: (key: string) => string;
  t_fmt: (key: string, args: Record<string, string>) => string;
  loading: boolean;
}

// ─── Context ─────────────────────────────────────────────────────────────────

const I18nContext = createContext<I18nContextValue>({
  locale: 'zh',
  t: (key: string) => key,
  t_fmt: (key: string) => key,
  loading: true,
});

// ─── Hook ────────────────────────────────────────────────────────────────────

export function useTranslation(): I18nContextValue {
  return useContext(I18nContext);
}

// ─── Provider ────────────────────────────────────────────────────────────────

export function I18nProvider({
  locale,
  children,
}: {
  locale: string;
  children: React.ReactNode;
}) {
  const [map, setMap] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const localeRef = useRef(locale);

  const loadTranslations = useCallback(async (loc: string) => {
    setLoading(true);
    try {
      const result = await rpc<TranslationsPayload>('hub/get_translations', { locale: loc });
      const entryMap: Record<string, string> = {};
      for (const { key, value } of result.entries) {
        entryMap[key] = value;
      }
      setMap(entryMap);
    } catch {
      // Fallback: use key as value when backend isn't available
      setMap({});
    }
    setLoading(false);
  }, []);

  // Load on mount
  useEffect(() => {
    loadTranslations(locale);
  }, [loadTranslations, locale]);

  // Reload when locale changes
  useEffect(() => {
    if (locale !== localeRef.current) {
      localeRef.current = locale;
      loadTranslations(locale);
    }
  }, [locale, loadTranslations]);

  const t = useCallback(
    (key: string): string => {
      return map[key] ?? key;
    },
    [map],
  );

  const t_fmt = useCallback(
    (key: string, args: Record<string, string>): string => {
      let result = map[key] ?? key;
      for (const [k, v] of Object.entries(args)) {
        result = result.replace(`{${k}}`, v);
      }
      return result;
    },
    [map],
  );

  return (
    <I18nContext.Provider value={{ locale, t, t_fmt, loading }}>
      {children}
    </I18nContext.Provider>
  );
}
