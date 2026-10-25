import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { errorMessage, type Preferences } from "@/lib/types";
type Theme = "light" | "dark" | "system";
const ThemeContext = createContext<{
  theme: Theme;
  setTheme: (theme: Theme) => void;
}>({ theme: "system", setTheme: () => {} });
export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, applyTheme] = useState<Theme>("system");
  const revision = useRef(0);
  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    const version = revision.current;
    void invoke<Preferences>("get_preferences")
      .then((prefs) => {
        if (active && revision.current === version && prefs)
          applyTheme(prefs.theme);
      })
      .catch(() => {}); // Bootstrap presents invalid-config errors in the main UI.
    return () => {
      active = false;
    };
  }, []);
  const setTheme = (next: Theme) => {
    const previous = theme;
    const version = ++revision.current;
    applyTheme(next);
    if (isTauri())
      void invoke("set_theme", { theme: next }).catch((error) => {
        if (revision.current === version) applyTheme(previous);
        toast.error(errorMessage(error));
      });
  };
  useLayoutEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      document.documentElement.classList.toggle(
        "dark",
        theme === "dark" || (theme === "system" && media.matches),
      );
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
  return (
    <ThemeContext.Provider value={{ theme, setTheme }}>
      {children}
    </ThemeContext.Provider>
  );
}
export const useTheme = () => useContext(ThemeContext);
