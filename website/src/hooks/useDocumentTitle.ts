import { useEffect } from "react";

export function useDocumentTitle(title: string): void {
  useEffect(() => {
    const original = document.title;
    document.title = title;
    return () => {
      document.title = original;
    };
  }, [title]);
}
