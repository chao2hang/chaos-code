import { cn } from "@/components/lib/utils.js";

export function ZCodeAboutLogo({ className }: { className?: string }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="118"
      height="100"
      fill="none"
      viewBox="0 0 256 256"
      className={cn("shrink-0 text-current", className)}
      aria-hidden="true"
      focusable="false"
    >
      <path
        fill="currentColor"
        d="M 202.86 69.51 A 95 95 0 0 0 33.13 123.03 L 65.09 124.70 A 63 63 0 0 1 177.64 89.21 Z"
      />
      <path
        fill="currentColor"
        d="M 202.86 186.49 A 95 95 0 0 1 33.13 132.97 L 65.09 131.30 A 63 63 0 0 0 177.64 166.79 Z"
      />
      <path
        fill="currentColor"
        d="M 105 92.5 L 140 128 L 105 163.5 L 122.5 163.5 L 157.5 128 L 122.5 92.5 Z"
      />
    </svg>
  );
}

export function ZCodeWordmarkLogo({ className }: { className?: string }) {
  return (
    <svg
      width="244"
      height="54"
      viewBox="0 0 244 54"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={cn("shrink-0 text-current", className)}
      aria-hidden="true"
      focusable="false"
    >
      <text
        x="0"
        y="38"
        fontFamily="Inter, -apple-system, BlinkMacSystemFont, sans-serif"
        fontWeight="800"
        fontSize="32"
        fill="currentColor"
        letterSpacing="2.5"
      >
        CHAOS
      </text>
    </svg>
  );
}
