import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { get, post, del } from "@/lib/api";

export interface EmailAccount {
  id: string;
  email: string;
  displayName: string;
  provider: string;
  isActive: boolean;
  lastError: string | null;
  lastSyncAt: string | null;
}

export interface ConnectEmailData {
  email: string;
  password: string;
  imapHost?: string;
  imapPort?: number;
  smtpHost?: string;
  smtpPort?: number;
  encryption?: string;
}

export const emailKeys = {
  all: ["email"] as const,
  accounts: () => [...emailKeys.all, "accounts"] as const,
  messages: (params?: Record<string, string>) => [...emailKeys.all, "messages", params] as const,
};

export function useEmailAccounts() {
  return useQuery<EmailAccount[]>({
    queryKey: emailKeys.accounts(),
    queryFn: () => get<EmailAccount[]>("/email/accounts"),
  });
}

export function useConnectEmail() {
  const queryClient = useQueryClient();
  return useMutation<{ message: string }, Error, ConnectEmailData>({
    mutationFn: (data) => post("/email/connect", data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: emailKeys.accounts() });
    },
  });
}

export function useDeleteEmailAccount() {
  const queryClient = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: (accountId) => del(`/email/accounts/${accountId}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: emailKeys.accounts() });
    },
  });
}

export function useSyncEmails() {
  const queryClient = useQueryClient();
  return useMutation<{ synced: number }, Error>({
    mutationFn: () => post("/email/sync"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: emailKeys.messages() });
    },
  });
}
