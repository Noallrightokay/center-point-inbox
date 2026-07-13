import { cn } from "@/lib/utils";
import { parseProvider, type ProviderType } from "@/lib/enums";
import type { Wire } from "@/types";

const STYLE: Record<ProviderType, { bg: string; letter: string; label: string }> = {
  GoogleWorkspace: { bg: "#4285F4", letter: "G", label: "Google Workspace" },
  Microsoft365: { bg: "#0F6CBD", letter: "M", label: "Microsoft 365" },
  Dropbox: { bg: "#0061FF", letter: "D", label: "Dropbox" },
};

export function ProviderIcon({
  provider,
  size = 20,
  className,
}: {
  provider: Wire | ProviderType;
  size?: number;
  className?: string;
}) {
  const key = parseProvider(provider);
  const s = STYLE[key];
  return (
    <span
      role="img"
      aria-label={s.label}
      title={s.label}
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded font-semibold text-white",
        className
      )}
      style={{
        width: size,
        height: size,
        backgroundColor: s.bg,
        fontSize: Math.round(size * 0.55),
      }}
    >
      {s.letter}
    </span>
  );
}
