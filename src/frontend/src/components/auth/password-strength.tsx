import { cn } from "@/lib/utils";

export const MIN_PASSWORD = 12;

export function scorePassword(pw: string): { score: number; label: string } {
  if (!pw) return { score: 0, label: "" };
  let score = 0;
  if (pw.length >= MIN_PASSWORD) score++;
  if (pw.length >= 16) score++;
  if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) score++;
  if (/\d/.test(pw)) score++;
  if (/[^A-Za-z0-9]/.test(pw)) score++;
  score = Math.min(score, 4);
  const labels = ["Very weak", "Weak", "Fair", "Good", "Strong"];
  return { score, label: labels[score] };
}

const BAR_COLOR = [
  "bg-destructive",
  "bg-destructive",
  "bg-warning",
  "bg-info",
  "bg-success",
];

export function PasswordStrength({ password }: { password: string }) {
  const { score, label } = scorePassword(password);
  const meetsMin = password.length >= MIN_PASSWORD;

  return (
    <div className="space-y-1.5">
      <div className="flex gap-1">
        {[0, 1, 2, 3].map((i) => (
          <span
            key={i}
            className={cn(
              "h-1 flex-1 rounded-full transition-colors",
              i < score ? BAR_COLOR[score] : "bg-muted"
            )}
          />
        ))}
      </div>
      <div className="flex items-center justify-between text-xs">
        <span
          className={cn(
            "text-muted-foreground",
            meetsMin ? "" : "text-destructive"
          )}
        >
          {meetsMin
            ? `${MIN_PASSWORD}+ characters`
            : `Minimum ${MIN_PASSWORD} characters`}
        </span>
        {label && <span className="text-muted-foreground">{label}</span>}
      </div>
    </div>
  );
}
