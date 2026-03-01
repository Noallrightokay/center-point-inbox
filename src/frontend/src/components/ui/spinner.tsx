import { Loader2 } from "lucide-react";

import { cn } from "@/lib/utils";

interface SpinnerProps extends React.HTMLAttributes<SVGSVGElement> {
  size?: number;
}

function Spinner({ className, size = 24, ...props }: SpinnerProps) {
  return (
    <Loader2
      className={cn("animate-spin text-muted-foreground", className)}
      size={size}
      {...props}
    />
  );
}

export { Spinner };
