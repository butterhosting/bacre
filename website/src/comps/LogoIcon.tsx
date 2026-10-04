import type { ComponentProps } from "react";

export function LogoIcon(props: ComponentProps<"svg">) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true" {...props}>
      <defs>
        <radialGradient id="bacre-crumb" cx="50%" cy="58%" r="60%">
          <stop offset="0" stopColor="#ead0a4" />
          <stop offset="1" stopColor="#f6e6cc" />
        </radialGradient>
      </defs>
      <path
        d="M4.6 11.6C2.6 10.2 2.5 5.6 6.3 4.1C8.3 3.3 10.5 3.6 12 4.8C13.5 3.6 15.7 3.3 17.7 4.1C21.5 5.6 21.4 10.2 19.4 11.6V19.6a1.8 1.8 0 0 1-1.8 1.8H6.4A1.8 1.8 0 0 1 4.6 19.6Z"
        fill="#c8894a"
      />
      <path
        d="M7 12.1C5.7 11.1 5.6 8 8 7C9.4 6.4 10.9 6.7 12 7.6C13.1 6.7 14.6 6.4 16 7C18.4 8 18.3 11.1 17 12.1V18.6a.6.6 0 0 1-.6.6H7.6a.6.6 0 0 1-.6-.6Z"
        fill="url(#bacre-crumb)"
      />
    </svg>
  );
}
