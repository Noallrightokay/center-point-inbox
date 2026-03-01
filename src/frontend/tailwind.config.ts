import type { Config } from "tailwindcss";

const config: Config = {
  darkMode: "class",
  content: [
    "./src/app/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/components/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/lib/**/*.{js,ts,jsx,tsx,mdx}",
  ],
  theme: {
    extend: {
      colors: {
        amex: {
          gold: "#B4975A",
          "gold-light": "#D4B87A",
          "gold-dark": "#8A7340",
          platinum: "#E8E8E8",
          black: "#0A0A0A",
          charcoal: "#1A1A2E",
          "charcoal-light": "#22223B",
          navy: "#16213E",
          slate: "#2C2C44",
          accent: "#C9A96E",
        },
        primary: {
          50: "#FBF7F0",
          100: "#F5ECD9",
          200: "#EBD9B3",
          300: "#D4B87A",
          400: "#C9A96E",
          500: "#B4975A",
          600: "#9A7E48",
          700: "#7D6639",
          800: "#614E2D",
          900: "#483A21",
          950: "#2E2515",
        },
        secondary: {
          50: "#F5F5F7",
          100: "#E8E8EC",
          200: "#D1D1D9",
          300: "#ADADBD",
          400: "#8A8AA0",
          500: "#6B6B84",
          600: "#55556C",
          700: "#464658",
          800: "#3B3B4B",
          900: "#2C2C3D",
          950: "#1A1A2E",
        },
      },
      fontFamily: {
        sans: ["var(--font-inter)", "system-ui", "sans-serif"],
        display: ["var(--font-playfair)", "Georgia", "serif"],
      },
      backgroundImage: {
        "amex-gradient": "linear-gradient(135deg, #1A1A2E 0%, #16213E 50%, #0F3460 100%)",
        "amex-gold": "linear-gradient(135deg, #B4975A 0%, #D4B87A 50%, #C9A96E 100%)",
        "amex-card": "linear-gradient(145deg, #22223B 0%, #1A1A2E 100%)",
        "amex-shine": "linear-gradient(120deg, transparent 0%, rgba(180, 151, 90, 0.1) 50%, transparent 100%)",
        "amex-dark": "linear-gradient(180deg, #0A0A0A 0%, #1A1A2E 50%, #16213E 100%)",
      },
      boxShadow: {
        amex: "0 4px 24px rgba(0, 0, 0, 0.4), 0 0 0 1px rgba(180, 151, 90, 0.1)",
        "amex-lg": "0 8px 40px rgba(0, 0, 0, 0.5), 0 0 0 1px rgba(180, 151, 90, 0.15)",
        "amex-glow": "0 0 20px rgba(180, 151, 90, 0.2), 0 4px 24px rgba(0, 0, 0, 0.4)",
        "amex-inner": "inset 0 1px 0 rgba(255, 255, 255, 0.05)",
      },
      keyframes: {
        "slide-in-right": {
          "0%": { transform: "translateX(100%)", opacity: "0" },
          "100%": { transform: "translateX(0)", opacity: "1" },
        },
        "slide-in-up": {
          "0%": { transform: "translateY(20px)", opacity: "0" },
          "100%": { transform: "translateY(0)", opacity: "1" },
        },
        "fade-in": {
          "0%": { opacity: "0" },
          "100%": { opacity: "1" },
        },
        shimmer: {
          "0%": { backgroundPosition: "-200% 0" },
          "100%": { backgroundPosition: "200% 0" },
        },
        "pulse-gold": {
          "0%, 100%": { boxShadow: "0 0 0 0 rgba(180, 151, 90, 0.4)" },
          "50%": { boxShadow: "0 0 0 8px rgba(180, 151, 90, 0)" },
        },
      },
      animation: {
        "slide-in-right": "slide-in-right 0.4s cubic-bezier(0.16, 1, 0.3, 1)",
        "slide-in-up": "slide-in-up 0.5s cubic-bezier(0.16, 1, 0.3, 1)",
        "fade-in": "fade-in 0.3s ease-out",
        shimmer: "shimmer 2s linear infinite",
        "pulse-gold": "pulse-gold 2s ease-in-out infinite",
      },
      borderRadius: {
        "2xl": "1rem",
        "3xl": "1.5rem",
      },
    },
  },
  plugins: [],
};

export default config;
