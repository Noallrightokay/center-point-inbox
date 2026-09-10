import { create } from "zustand";

type Theme = "light" | "dark";

const STORAGE_KEY = "centra-theme";

function applyTheme(theme: Theme) {
  if (typeof document === "undefined") return;
  const root = document.documentElement.classList;
  theme === "dark" ? root.add("dark") : root.remove("dark");
}

function readInitial(): Theme {
  if (typeof document === "undefined") return "dark";
  return document.documentElement.classList.contains("dark") ? "dark" : "light";
}

interface ThemeState {
  theme: Theme;
  toggle: () => void;
  setTheme: (t: Theme) => void;
  /** Sync store state with whatever the pre-paint script already applied. */
  hydrate: () => void;
}

export const useThemeStore = create<ThemeState>((set, get) => ({
  theme: "dark",

  hydrate: () => set({ theme: readInitial() }),

  toggle: () => {
    const next: Theme = get().theme === "dark" ? "light" : "dark";
    get().setTheme(next);
  },

  setTheme: (t) => {
    applyTheme(t);
    try {
      localStorage.setItem(STORAGE_KEY, t);
    } catch {
      /* ignore */
    }
    set({ theme: t });
  },
}));
