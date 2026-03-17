/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        "space-bg": "#0a0a0a",
        "space-secondary": "#0d0d1a",
        "space-border": "#1a1a2e",
        "neon": "#00d4ff",
      },
      fontFamily: {
        mono: [
          "JetBrains Mono",
          "Fira Code",
          "Cascadia Code",
          "Consolas",
          "monospace",
        ],
      },
      boxShadow: {
        neon: "0 0 12px rgba(0, 212, 255, 0.4), 0 0 24px rgba(0, 212, 255, 0.15)",
        "neon-sm": "0 0 6px rgba(0, 212, 255, 0.3)",
      },
      animation: {
        "pulse-neon": "pulse-neon 2s cubic-bezier(0.4, 0, 0.6, 1) infinite",
      },
      keyframes: {
        "pulse-neon": {
          "0%, 100%": { opacity: 1 },
          "50%": { opacity: 0.4 },
        },
      },
    },
  },
  plugins: [],
};
