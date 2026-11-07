import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";
import jsxA11y from "eslint-plugin-jsx-a11y";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  {
    files: ["**/*.{ts,tsx,mts,cts}"],
    extends: [tseslint.configs.recommendedTypeChecked],
    languageOptions: {
      parserOptions: {
        // The Playwright configs sit beside the app but are type-checked with the browser tests (e2e/tsconfig.json),
        // out of the app's tsconfig, whose files the web image's build type-checks.
        projectService: {
          allowDefaultProject: ["playwright*.config.ts"],
          defaultProject: "e2e/tsconfig.json",
        },
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      "@typescript-eslint/no-explicit-any": "error",
    },
  },
  {
    rules: {
      ...jsxA11y.flatConfigs.recommended.rules,
      ...reactHooks.configs.flat.recommended.rules,
      // Labelled scroll regions need a tab stop for keyboard scrolling, and so do the labelled groups a page repeats
      // (a tool card's Result, Output and arguments), which are groups so they never repeat a landmark.
      "jsx-a11y/no-noninteractive-tabindex": [
        "error",
        { roles: ["region", "group"] },
      ],
      "import/order": [
        "error",
        {
          groups: [
            "builtin",
            "external",
            "internal",
            "parent",
            "sibling",
            "index",
          ],
          "newlines-between": "always",
          alphabetize: { order: "asc", caseInsensitive: true },
        },
      ],
    },
  },
  globalIgnores([
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
    "coverage/**",
    "playwright-report/**",
    "test-results/**",
    "**/generated/**",
  ]),
]);

export default eslintConfig;
