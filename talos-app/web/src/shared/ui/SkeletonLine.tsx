import { cn } from "@/shared/lib/cn";

interface SkeletonLineProps {
  className?: string;
}

export function SkeletonLine({ className }: SkeletonLineProps) {
  return <div className={cn("animate-pulse rounded bg-surface/80", className)} />;
}
