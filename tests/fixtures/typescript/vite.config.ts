// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

/// <reference types="vitest/config" />
import { defineConfig } from "vite";

// https://vite.dev/config/
export default defineConfig({
    test: {
        coverage: {
            // Report every source file, even those no test imports, and print the report rather than write files.
            include: ["src/**"],
            reporter: ["text"],
        },
    },
});
