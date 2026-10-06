import { useLayoutEffect } from "react";
import type { Appearance } from "./types";

export function useAppearance(appearance: Appearance = "notebook") {
  useLayoutEffect(() => {
    document.documentElement.dataset.appearance = appearance;
  }, [appearance]);
}
