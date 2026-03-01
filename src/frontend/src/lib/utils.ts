import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * Combines clsx and tailwind-merge for conditional class name composition.
 * Handles Tailwind CSS class conflicts intelligently.
 */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
