// The fixed links of Settings > About; the same URLs as AboutLink in src-tauri/src/commands.rs.
import type { AboutLink } from "./types";

export const ABOUT_URLS: Record<AboutLink, string> = {
  repository: "https://github.com/trimcraft/trimcraft",
  issues: "https://github.com/trimcraft/trimcraft/issues/new/choose",
  license: "https://www.gnu.org/licenses/gpl-3.0.html",
};
