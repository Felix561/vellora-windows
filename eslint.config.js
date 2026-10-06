import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";

export default [
  { ignores: ["dist/**", "src-tauri/**", "node_modules/**", "coverage/**", ".vitest/**"] },
  {
    files: ["src/**/*.{ts,tsx}", "tests/frontend/**/*.{ts,tsx}", "vitest.config.ts"],
    ...js.configs.recommended,
    languageOptions: { parser: tseslint.parser },
    plugins: { "@typescript-eslint": tseslint.plugin, "react-hooks": reactHooks },
    rules: {
      ...tseslint.configs.recommended.reduce((rules, config) => ({ ...rules, ...config.rules }), {}),
      "no-undef": "off", // TypeScript checks globals and imported names.
      "no-unused-vars": "off",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }],
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
      eqeqeq: ["error", "always"],
      "no-var": "error",
      "prefer-const": "error",
    },
  },
];
