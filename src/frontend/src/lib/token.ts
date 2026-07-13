// In-memory + localStorage JWT holder. Kept separate from the axios client and
// the auth store so both can read/write it without a circular import.

const STORAGE_KEY = "centra-token";

let memoryToken: string | null = null;

export function getToken(): string | null {
  if (memoryToken) return memoryToken;
  if (typeof window === "undefined") return null;
  try {
    memoryToken = window.localStorage.getItem(STORAGE_KEY);
  } catch {
    memoryToken = null;
  }
  return memoryToken;
}

export function setToken(token: string): void {
  memoryToken = token;
  if (typeof window !== "undefined") {
    try {
      window.localStorage.setItem(STORAGE_KEY, token);
    } catch {
      /* storage unavailable — memory token still works for this tab */
    }
  }
}

export function clearToken(): void {
  memoryToken = null;
  if (typeof window !== "undefined") {
    try {
      window.localStorage.removeItem(STORAGE_KEY);
    } catch {
      /* ignore */
    }
  }
}
