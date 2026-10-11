import type { PlainTextConversionSettings } from "@/types/settings";

/** Mirrors the Rust conversion rule count limit, including disabled rules. */
export const MAX_CUSTOM_RULES = 32;

/**
 * Mirrors Rust defaults and gives each editor an independent rule array.
 */
export const createDefaultPlainTextConversion =
  (): PlainTextConversionSettings => {
    return {
      collapseBlankLines: false,
      imageStyle: "alt",
      linkStyle: "title",
      listStyle: "textOnly",
      rules: [],
      tableSeparator: "tab",
      taskStyle: "status",
    };
  };
