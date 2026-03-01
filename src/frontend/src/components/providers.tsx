"use client";

import { useState } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

function makeQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: 60 * 1000,
        retry: (failureCount, error) => {
          // Do not retry on 401 (auth) or 403 (forbidden) errors.
          if (
            error &&
            typeof error === "object" &&
            "status" in error
          ) {
            const status = (error as { status: number }).status;
            if (status === 401 || status === 403) {
              return false;
            }
          }
          return failureCount < 3;
        },
        refetchOnWindowFocus: false,
      },
      mutations: {
        retry: false,
      },
    },
  });
}

let browserQueryClient: QueryClient | undefined;

function getQueryClient(): QueryClient {
  if (typeof window === "undefined") {
    // Server: always create a new QueryClient.
    return makeQueryClient();
  }
  // Browser: reuse the same QueryClient across renders.
  if (!browserQueryClient) {
    browserQueryClient = makeQueryClient();
  }
  return browserQueryClient;
}

export function ClientProviders({ children }: { children: React.ReactNode }) {
  const [queryClient] = useState(getQueryClient);

  return (
    <QueryClientProvider client={queryClient}>
      {children}
    </QueryClientProvider>
  );
}
