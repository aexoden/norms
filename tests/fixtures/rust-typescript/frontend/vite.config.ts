// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

/// <reference types="vitest/config" />
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { defineConfig } from "vite";

/**
 * The version of this build, from the release tags: `git describe`'s without the `v`, such as `0.2.0` on a release
 * tag or `0.2.0-5-gf8a4d2b` past one (ending in `-dirty` if the working tree has changes), the package's version and
 * the commit in a clone without tags (`0.2.0+gf8a4d2b`), or the package's version alone without git.
 */
function buildVersion(): string {
    const { version } = JSON.parse(readFileSync(join(import.meta.dirname, "package.json"), "utf8")) as {
        version: string;
    };
    try {
        const describe = execFileSync("git", ["describe", "--tags", "--match=v[0-9]*", "--always", "--dirty"], {
            cwd: import.meta.dirname,
            encoding: "utf8",
            stdio: ["ignore", "pipe", "ignore"],
        }).trim();
        return describe.startsWith("v") ? describe.slice(1) : `${version}+g${describe}`;
    } catch {
        return version;
    }
}

// https://vite.dev/config/
export default defineConfig({
    define: {
        __APP_VERSION__: JSON.stringify(buildVersion()),
    },
    test: {
        coverage: {
            // Report every source file, even those no test imports, and print the report rather than write files.
            include: ["src/**"],
            reporter: ["text"],
        },
    },
});
