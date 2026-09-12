import type { Config } from "tailwindcss";

/**
 * Токены из макета Claude Design («Restyle UI.dc.html»): IBM Plex Sans для
 * интерфейса, IBM Plex Mono для клавиш, хоткеев и имён процессов; акцент —
 * янтарный (не путается с синим системным выделением Windows).
 * Тёмные и светлые значения разведены: у акцента в светлой теме свой оттенок.
 */
export default {
  // Тема ставится атрибутом на <html> (см. src/lib/theme.ts), а не классом.
  // Именно `selector`, а не вариант с `:where()`: `:where()` даёт нулевую
  // специфичность, и базовый (светлый) класс перебивал бы `dark:`.
  darkMode: ["selector", '[data-theme="dark"]'],
  content: ["./index.html", "./settings.html", "./history.html", "./welcome.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        sans: ["'IBM Plex Sans'", "system-ui", "Segoe UI", "sans-serif"],
        mono: ["'IBM Plex Mono'", "ui-monospace", "Consolas", "monospace"],
      },
      colors: {
        // Акцент. Имя `amber` историческое: цвет задаёт пользователь (вкладка
        // «Интерфейс»), значения — CSS-переменные «R G B», чтобы работали
        // модификаторы прозрачности вроде `bg-amber/[.14]`. См. src/lib/theme.ts.
        amber: {
          DEFAULT: "rgb(var(--amber) / <alpha-value>)", // тёмная тема
          soft: "rgb(var(--amber-soft) / <alpha-value>)",
          deep: "rgb(var(--amber-deep) / <alpha-value>)", // светлая тема
          ink: "rgb(var(--amber-ink) / <alpha-value>)", // текст по светлому фону
          mid: "rgb(var(--amber-mid) / <alpha-value>)",
          on: "rgb(var(--amber-on) / <alpha-value>)", // текст на заливке акцентом
        },
        danger: { DEFAULT: "#f2756a", ink: "#b8443a" },
        ok: "#3f8f5e",
        // тёмные поверхности
        ink: {
          900: "#0d0e10",
          800: "#17181c",
          750: "#1b1d21",
          700: "#1e2024",
          600: "#23262b",
          text: "#edeef0",
        },
        // светлые поверхности
        paper: {
          DEFAULT: "#f4f2ef",
          sand: "#eeebe6",
          bar: "#eae7e2",
          line: "#f0ede8",
          text: "#16181c",
          dim: "#6d7178",
          mid: "#4b4f56",
          soft: "#73777e",
        },
      },
      borderRadius: { hud: "12px" },
      boxShadow: {
        hud: "0 18px 44px rgba(0,0,0,.55), 0 0 0 1px rgb(var(--amber) / .1)",
        hudLight: "0 18px 44px rgba(0,0,0,.22), 0 2px 6px rgba(0,0,0,.1)",
        toast: "0 12px 30px rgba(0,0,0,.45)",
        card: "0 1px 2px rgba(0,0,0,.07)",
      },
      keyframes: {
        caret: { "0%,49%": { opacity: "1" }, "50%,100%": { opacity: "0" } },
        pulse2: { "0%,100%": { opacity: ".35" }, "50%": { opacity: ".9" } },
      },
      animation: {
        caret: "caret 1.1s steps(1) infinite",
        pulse2: "pulse2 1.2s ease-in-out infinite",
      },
    },
  },
  plugins: [],
} satisfies Config;
