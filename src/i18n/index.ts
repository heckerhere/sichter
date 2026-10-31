// Mehrsprachigkeit (Deutsch/Englisch). Texte liegen als typgeprüfte Kataloge in
// ./de und ./en; fehlt im englischen Katalog ein Eintrag, meldet TypeScript das.
//
// Verwendung: `import { t } from "../i18n"`, im Template `{{ t.library.title }}`
// bzw. mit Parametern `{{ t.library.files(n) }}`. `t` ist reaktiv: Beim
// Umschalten der Sprache werden alle Ansichten neu gezeichnet.
import { ref, watch } from "vue";
import { de } from "./de";
import { en } from "./en";

export type Locale = "de" | "en";
export type LanguagePref = "auto" | Locale;
export type Messages = typeof de;

const catalogs: Record<Locale, Messages> = { de, en };

/** Sprache des Systems: Deutsch, wenn die bevorzugte Sprache Deutsch ist, sonst Englisch. */
export function systemLocale(): Locale {
  const first = (navigator.languages?.[0] ?? navigator.language ?? "en").toLowerCase();
  return first.startsWith("de") ? "de" : "en";
}

export const locale = ref<Locale>(systemLocale());

export const resolveLocale = (pref: LanguagePref): Locale => (pref === "auto" ? systemLocale() : pref);

/** Aktueller Katalog. Jeder Zugriff liest `locale` und ist damit reaktiv. */
export const t = new Proxy({} as Messages, {
  get: (_, key) => catalogs[locale.value][key as keyof Messages],
});

/** BCP-47-Tag für Intl (Zahlen, Datumsangaben). */
export function intlTag(): string {
  if (locale.value === "de") return "de-DE";
  const sys = navigator.language ?? "en-US";
  return sys.toLowerCase().startsWith("en") ? sys : "en-US";
}

watch(locale, (l) => (document.documentElement.lang = l), { immediate: true });
