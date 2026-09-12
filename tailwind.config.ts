import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./settings.html", "./src/**/*.{ts,tsx}"],
  theme: { extend: {} },
  plugins: [],
} satisfies Config;
