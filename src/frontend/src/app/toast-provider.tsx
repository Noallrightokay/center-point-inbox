"use client";

import * as React from "react";
import * as ToastPrimitive from "@radix-ui/react-toast";

interface ToastContextValue {
  toast: (options: ToastOptions) => void;
}

interface ToastOptions {
  title: string;
  description?: string;
  variant?: "default" | "success" | "error" | "warning";
  duration?: number;
}

interface ToastItem extends ToastOptions {
  id: string;
  open: boolean;
}

const ToastContext = React.createContext<ToastContextValue | undefined>(
  undefined
);

export function useToast(): ToastContextValue {
  const context = React.useContext(ToastContext);
  if (!context) {
    throw new Error("useToast must be used within a ToastProvider");
  }
  return context;
}

const variantStyles: Record<string, string> = {
  default:
    "bg-white dark:bg-secondary-800 border-secondary-200 dark:border-secondary-700",
  success:
    "bg-green-50 dark:bg-green-950 border-green-200 dark:border-green-800",
  error: "bg-red-50 dark:bg-red-950 border-red-200 dark:border-red-800",
  warning:
    "bg-yellow-50 dark:bg-yellow-950 border-yellow-200 dark:border-yellow-800",
};

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = React.useState<ToastItem[]>([]);

  const toast = React.useCallback((options: ToastOptions) => {
    const id = Math.random().toString(36).substring(2, 9);
    setToasts((prev) => [...prev, { ...options, id, open: true }]);
  }, []);

  const handleOpenChange = React.useCallback((id: string, open: boolean) => {
    if (!open) {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }
  }, []);

  return (
    <ToastContext.Provider value={{ toast }}>
      <ToastPrimitive.Provider swipeDirection="right">
        {children}
        {toasts.map((t) => (
          <ToastPrimitive.Root
            key={t.id}
            open={t.open}
            onOpenChange={(open) => handleOpenChange(t.id, open)}
            duration={t.duration ?? 5000}
            className={`rounded-lg border p-4 shadow-lg animate-slide-in-right ${
              variantStyles[t.variant ?? "default"]
            }`}
          >
            <ToastPrimitive.Title className="text-sm font-semibold text-foreground">
              {t.title}
            </ToastPrimitive.Title>
            {t.description && (
              <ToastPrimitive.Description className="mt-1 text-sm text-muted-foreground">
                {t.description}
              </ToastPrimitive.Description>
            )}
            <ToastPrimitive.Close
              aria-label="Close"
              className="absolute right-2 top-2 rounded-md p-1 text-secondary-400 hover:text-secondary-600 dark:hover:text-secondary-300"
            >
              <span aria-hidden>x</span>
            </ToastPrimitive.Close>
          </ToastPrimitive.Root>
        ))}
        <ToastPrimitive.Viewport className="fixed bottom-0 right-0 z-[100] flex max-h-screen w-full flex-col-reverse gap-2 p-4 sm:max-w-[420px]" />
      </ToastPrimitive.Provider>
    </ToastContext.Provider>
  );
}
