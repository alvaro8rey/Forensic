import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

import en from "./locales/en.json";
import es from "./locales/es.json";

export const SUPPORTED_LANGUAGES = [
  { code: "en", label: "English", nativeLabel: "English" },
  { code: "es", label: "Spanish", nativeLabel: "Español" },
] as const;

export type LangCode = "en" | "es";

/** localStorage key used to persist the user's language choice */
export const LANG_STORAGE_KEY = "aeon_language";

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      es: { translation: es },
    },
    fallbackLng: "en",
    supportedLngs: ["en", "es"],
    // LanguageDetector checks these sources in order:
    // 1. localStorage key "aeon_language" (persisted user choice)
    // 2. navigator.language (OS/browser locale — acts as installer-language proxy)
    detection: {
      order: ["localStorage", "navigator"],
      lookupLocalStorage: LANG_STORAGE_KEY,
      caches: ["localStorage"],
      // Normalise "es-ES", "es-MX", etc. → "es"
      convertDetectedLanguage: (lng: string) => lng.split("-")[0],
    },
    interpolation: {
      escapeValue: false,
    },
  });

export default i18n;
